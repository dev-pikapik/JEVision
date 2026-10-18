import JEVisionCore
import JEVisionHands
import SwiftUI

/// A schedule as the editor shows it: every choice keeps its value while another one is
/// picked, so switching back and forth loses nothing.
struct ScheduleDraft: Equatable {
  enum Tests: Hashable { case all, folder, files }
  enum Repeat: Hashable { case daily, weekdays, days, hours }

  /// Name of the schedule being changed; `nil` for a new one.
  var originalName: String?
  var name: String
  var tests = Tests.all
  var folder = ""
  var files: Set<String> = []
  var repeats = Repeat.weekdays
  var days = Set(ScheduleText.weekdays)
  var hours: UInt8 = 3
  var hour: UInt8 = 9
  var minute: UInt8 = 0
  var environment: String?
  /// `nil` follows Settings → Advanced.
  var video: VideoMode?

  init(name: String, environment: String?) {
    self.name = name
    self.environment = environment
  }

  init(_ schedule: ProjectSchedule) {
    originalName = schedule.name
    name = schedule.name
    switch schedule.tests {
    case .all: tests = .all
    case .folder(let path): (tests, folder) = (.folder, path)
    case .files(let paths): (tests, files) = (.files, Set(paths))
    }
    switch schedule.repeats {
    case .daily: repeats = .daily
    case .days(let chosen) where Set(chosen) == Set(ScheduleText.weekdays): repeats = .weekdays
    case .days(let chosen): (repeats, days) = (.days, Set(chosen))
    case .everyHours(let every): (repeats, hours) = (.hours, every)
    }
    (hour, minute) = (schedule.hour, schedule.minute)
    environment = schedule.environment
    video = schedule.video
  }

  var schedule: ProjectSchedule {
    let tests: ScheduledTests =
      switch self.tests {
      case .all: .all
      case .folder: .folder(path: folder)
      case .files: .files(paths: files.sorted())
      }
    let repeats: ScheduleRepeat =
      switch self.repeats {
      case .daily: .daily
      case .weekdays: .days(days: Data(ScheduleText.weekdays))
      case .days: .days(days: Data(days.sorted()))
      case .hours: .everyHours(hours: hours)
      }
    return ProjectSchedule(
      name: name.trimmingCharacters(in: .whitespaces), tests: tests, repeats: repeats, hour: hour, minute: minute,
      environment: environment, video: video)
  }

  /// The time of day as a date today, for the time picker.
  var time: Date {
    get { Calendar.current.date(from: DateComponents(hour: Int(hour), minute: Int(minute))) ?? .now }
    set {
      let parts = Calendar.current.dateComponents([.hour, .minute], from: newValue)
      (hour, minute) = (UInt8(parts.hour ?? 0), UInt8(parts.minute ?? 0))
    }
  }
}

struct ScheduleEditor: View {
  @State var draft: ScheduleDraft
  /// The project's other schedules, whose names this one must not take.
  let others: [ProjectSchedule]
  let save: (ProjectSchedule) -> Void
  let cancel: () -> Void
  @Environment(ProjectStore.self) private var project

  private var problem: String? {
    switch draft.tests {
    case .folder where draft.folder.isEmpty: return String(localized: "Choose a folder.")
    case .files where draft.files.isEmpty: return String(localized: "Choose at least one test.")
    default: break
    }
    if draft.repeats == .days, draft.days.isEmpty { return String(localized: "Choose at least one day.") }
    if draft.name.trimmingCharacters(in: .whitespaces).isEmpty { return String(localized: "Give the schedule a name.") }
    if others.contains(where: { $0.name.lowercased() == draft.schedule.name.lowercased() }) {
      return String(localized: "Another schedule has this name.")
    }
    return scheduleProblem(schedule: draft.schedule, others: others)
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      (draft.originalName == nil ? Text("New Schedule") : Text("Edit Schedule"))
        .font(.title2.bold())
        .accessibilityAddTraits(.isHeader)
      Form {
        TextField(text: $draft.name, prompt: Text("Weekday Mornings")) { Text("Name") }
        Section {
          Picker("Tests", selection: $draft.tests) {
            Text("All tests").tag(ScheduleDraft.Tests.all)
            Text("Tests in a folder").tag(ScheduleDraft.Tests.folder)
            Text("Chosen tests").tag(ScheduleDraft.Tests.files)
          }
          if draft.tests == .folder { folderPicker }
          if draft.tests == .files { testList }
        }
        Section {
          Picker("Repeat", selection: $draft.repeats) {
            Text("Every day").tag(ScheduleDraft.Repeat.daily)
            Text("Every weekday").tag(ScheduleDraft.Repeat.weekdays)
            Text("On chosen days").tag(ScheduleDraft.Repeat.days)
            Text("Every few hours").tag(ScheduleDraft.Repeat.hours)
          }
          if draft.repeats == .days { dayPicker }
          if draft.repeats == .hours {
            Picker("How often", selection: $draft.hours) {
              ForEach(scheduleEveryHours(), id: \.self) { hours in
                hours == 1 ? Text("Every hour").tag(hours) : Text("Every \(Int(hours)) hours").tag(hours)
              }
            }
          }
          DatePicker(selection: $draft.time, displayedComponents: .hourAndMinute) {
            draft.repeats == .hours ? Text("Starting at") : Text("At")
          }
        }
        Section {
          environmentPicker
          Picker("Record video", selection: $draft.video) {
            Text("As in Settings").tag(VideoMode?.none)
            ForEach(VideoMode.all, id: \.name) { Text(AdvancedSettingsView.title($0)).tag(VideoMode?.some($0)) }
          }
        }
      }
      .formStyle(.grouped)
      .frame(height: 400)
      summary
      HStack {
        Spacer()
        Button("Cancel", role: .cancel, action: cancel)
          .keyboardShortcut(.cancelAction)
        Button("Save") { save(draft.schedule) }
          .keyboardShortcut(.defaultAction)
          .disabled(problem != nil)
      }
    }
  }

  /// The schedule in words and its next run, or what keeps it from being saved.
  @ViewBuilder private var summary: some View {
    if let problem {
      Label(problem, systemImage: "exclamationmark.circle")
        .foregroundStyle(.secondary)
    } else {
      let schedule = draft.schedule
      VStack(alignment: .leading, spacing: 2) {
        Text(verbatim: "\(ScheduleText.summary(schedule)) · \(ScheduleText.tests(schedule.tests))")
        if let next = ProjectSchedules.nextRun(of: schedule) {
          Text("Next run \(ScheduleText.moment(next))").foregroundStyle(.secondary)
        }
      }
    }
  }

  private var folders: [String] {
    func walk(_ items: [ProjectItem]) -> [String] {
      items.filter { $0.kind == .folder }.flatMap { [$0.id] + walk($0.children) }
    }
    return walk(project.items)
  }

  @ViewBuilder private var folderPicker: some View {
    let folders = folders
    if folders.isEmpty {
      Text("The project has no folders with tests.").foregroundStyle(.secondary)
    } else {
      Picker("Folder", selection: $draft.folder) {
        if !folders.contains(draft.folder) { Text("Choose…").tag(draft.folder) }
        ForEach(folders, id: \.self) { Text(verbatim: $0).tag($0) }
      }
    }
  }

  private var testList: some View {
    let tests = project.items.flatMap(\.tests)
    return ScrollView {
      VStack(alignment: .leading, spacing: 6) {
        ForEach(tests) { test in
          Toggle(
            isOn: Binding(
              get: { draft.files.contains(test.id) },
              set: { if $0 { draft.files.insert(test.id) } else { draft.files.remove(test.id) } })
          ) {
            Text(verbatim: test.parentPath.isEmpty ? test.name : "\(test.parentPath)/\(test.name)")
          }
        }
      }
      .frame(maxWidth: .infinity, alignment: .leading)
    }
    .frame(height: 140)
  }

  private var dayPicker: some View {
    let symbols = Calendar.current.shortWeekdaySymbols
    let names = Calendar.current.weekdaySymbols
    return HStack(spacing: 4) {
      ForEach(ScheduleText.orderedDays, id: \.self) { day in
        Toggle(
          isOn: Binding(
            get: { draft.days.contains(day) },
            set: { if $0 { draft.days.insert(day) } else { draft.days.remove(day) } })
        ) {
          Text(verbatim: symbols[Int(day)])
        }
        .toggleStyle(.button)
        .accessibilityLabel(Text(verbatim: names[Int(day)]))
      }
    }
  }

  @ViewBuilder private var environmentPicker: some View {
    let environments = project.environments.current.all.map(\.name)
    if environments.isEmpty, draft.environment == nil {
      LabeledContent("Environment") { Text("The project has none").foregroundStyle(.secondary) }
    } else {
      Picker("Environment", selection: $draft.environment) {
        Text("Project default").tag(String?.none)
        ForEach(Array(Set(environments + [draft.environment].compactMap(\.self))).sorted(), id: \.self) {
          Text(verbatim: $0).tag(String?.some($0))
        }
      }
    }
  }
}
