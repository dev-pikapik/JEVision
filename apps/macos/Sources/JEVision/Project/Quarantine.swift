import JEVisionCore
import SwiftUI

/// A test about to go to quarantine, or to have its quarantine changed.
struct QuarantineRequest: Identifiable {
  /// Test file, relative to the project.
  let file: String
  let name: String
  /// The quarantine the test is in now, for the sheet to start from.
  var entry: Quarantined?
  var id: String { file }
}

/// The days of a quarantine as the project settings file writes them (`YYYY-MM-DD`) and as a
/// date picker holds them.
enum QuarantineDay {
  static func date(_ day: String) -> Date? {
    let parts = day.split(separator: "-").compactMap { Int($0) }
    guard parts.count == 3 else { return nil }
    return Calendar.current.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2]))
  }

  static func string(_ date: Date) -> String {
    let parts = Calendar.current.dateComponents([.year, .month, .day], from: date)
    return String(format: "%04d-%02d-%02d", parts.year ?? 0, parts.month ?? 0, parts.day ?? 0)
  }

  /// How the day reads in the app, such as "Nov 3, 2026".
  static func title(_ day: String) -> String {
    date(day).map { $0.formatted(date: .abbreviated, time: .omitted) } ?? day
  }
}

/// What a person types and picks to put a test in quarantine.
struct QuarantineDraft: Equatable {
  var reason = ""
  /// The test comes back to the results on a day, instead of staying until it is released.
  var returns = true
  var until: Date

  /// Starts from the quarantine the test is in, or from the usual two weeks.
  init(_ entry: Quarantined? = nil) {
    reason = entry?.reason ?? ""
    returns = entry == nil || entry?.until != nil
    until =
      entry?.until.flatMap(QuarantineDay.date) ?? defaultQuarantineEnd(today: localToday()).flatMap(QuarantineDay.date) ?? .now
  }

  /// The last day of the quarantine, or `nil` for one without an end.
  var lastDay: String? { returns ? QuarantineDay.string(until) : nil }
}

enum QuarantineText {
  /// "In quarantine" with its end and reason, for marks and their help; once its last day is
  /// past, that the quarantine ended.
  static func describe(_ entry: Quarantined, today: String = localToday()) -> String {
    guard let until = entry.until.map(QuarantineDay.title) else {
      return entry.reason.map { String(localized: "In quarantine: \($0)") } ?? String(localized: "In quarantine")
    }
    guard quarantineActive(entry: entry, today: today) else {
      return entry.reason.map { String(localized: "Quarantine ended \(until): \($0)") }
        ?? String(localized: "Quarantine ended \(until)")
    }
    return entry.reason.map { String(localized: "In quarantine until \(until): \($0)") }
      ?? String(localized: "In quarantine until \(until)")
  }

  /// What the quarantine meant for a run of a test that was in it.
  static func describe(_ report: RunReport) -> String {
    let entry = describe(Quarantined(test: report.file, reason: report.quarantineReason))
    return report.verdict == .failed
      ? entry + ". " + String(localized: "The failure doesn’t fail Run All, the command line or the JUnit report.")
      : entry
  }

  static let explanation = String(
    localized:
      "A test in quarantine still runs, but its failures don’t fail Run All, the command line or the JUnit report. The quarantine is saved in the project, so everyone who runs its tests sees it."
  )

  /// Why quarantine is suggested, also the reason it is saved with.
  static func suggestion(_ suggestion: QuarantineSuggestion) -> String {
    String(localized: "Unreliable \(suggestion.events.count) times in the last 7 days")
  }

  /// One unreliable run, such as "Oct 17, 14:30 · Failed after passing".
  static func event(_ event: FlakyEvent) -> String {
    let time = RunHistory.date(event.startedAt).formatted(date: .abbreviated, time: .shortened)
    let what =
      switch event.kind {
      case .passedOnRetry: String(localized: "Passed only on a retry")
      case .failedAfterPass: String(localized: "Failed after passing, nothing changed")
      }
    return time + " · " + what
  }
}

/// The mark of a test in quarantine, with the reason as its help.
struct QuarantineMark: View {
  let entry: Quarantined

  var body: some View {
    Image(systemName: RunStatus.quarantined.symbol)
      .foregroundStyle(RunStatus.quarantined.color)
      .help(Text(QuarantineText.describe(entry)))
      .accessibilityLabel(Text(QuarantineText.describe(entry)))
  }
}

/// The fields of a quarantine: why, and the day the test comes back to the results.
struct QuarantineFields: View {
  @Binding var draft: QuarantineDraft

  var body: some View {
    TextField(text: $draft.reason, prompt: Text("Reason, such as a link to the bug")) { Text("Reason") }
    Toggle("Come back to the results on a day", isOn: $draft.returns)
    if draft.returns {
      DatePicker(
        "In quarantine until", selection: $draft.until, in: Calendar.current.startOfDay(for: .now)..., displayedComponents: .date)
    }
  }

  /// What happens when the quarantine ends, or doesn't.
  static func footnote(_ draft: QuarantineDraft) -> Text {
    draft.returns
      ? Text(
        "The day after \(draft.until.formatted(date: .abbreviated, time: .omitted)) the test counts again: its failures fail Run All, the command line and the JUnit report.")
      : Text("The test stays in quarantine until you release it.")
  }
}

/// Asks why and until when the test goes to quarantine, then puts it there.
struct QuarantineSheet: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  let request: QuarantineRequest
  @State private var draft: QuarantineDraft
  @State private var failure: String?

  init(request: QuarantineRequest) {
    self.request = request
    _draft = State(initialValue: QuarantineDraft(request.entry))
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 16) {
      Text("Quarantine “\(request.name)”")
        .font(.title2.bold())
        .accessibilityAddTraits(.isHeader)
      Text(QuarantineText.explanation)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      Form { QuarantineFields(draft: $draft) }
        .formStyle(.columns)
      QuarantineFields.footnote(draft)
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      if let failure {
        Text(failure).foregroundStyle(.red).font(.callout)
      }
      HStack {
        Spacer()
        Button("Cancel", role: .cancel) { dismiss() }
          .keyboardShortcut(.cancelAction)
        Button(request.entry == nil ? "Put in Quarantine" : "Save") {
          do {
            try project.quarantine(request.file, reason: draft.reason, until: draft.lastDay)
            dismiss()
          } catch {
            failure = ProjectStore.message(error)
          }
        }
        .keyboardShortcut(.defaultAction)
      }
    }
    .padding(20)
    .frame(width: 420)
  }
}

extension View {
  /// Shows the Quarantine sheet while `request` is set.
  func quarantineSheet(_ request: Binding<QuarantineRequest?>) -> some View {
    sheet(item: request) { QuarantineSheet(request: $0) }
  }
}

/// Why quarantine is suggested for a test, which runs show it, and the one button that does it.
struct QuarantineSuggestionView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  let file: String
  let suggestion: QuarantineSuggestion
  let failed: (String) -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      Label {
        Text(QuarantineText.suggestion(suggestion)).bold()
      } icon: {
        Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
      }
      Text("Nothing in the test changed, yet its result did. In quarantine its failures stop failing Run All while you look into it.")
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      ForEach(suggestion.events, id: \.startedAt) { event in
        if let run = project.runs.first(where: { $0.report.file == file && $0.report.startedAt == event.startedAt }) {
          Button {
            project.showRun(run.url)
            dismiss()
          } label: {
            Label(QuarantineText.event(event), systemImage: "clock.arrow.circlepath")
          }
          .buttonStyle(.link)
          .help(Text("Open the report of this run"))
        } else {
          Label(QuarantineText.event(event), systemImage: "clock.arrow.circlepath")
            .foregroundStyle(.secondary)
        }
      }
      Button("Put in Quarantine until \(QuarantineDay.title(defaultQuarantineEnd(today: localToday()) ?? localToday()))") {
        do { try project.quarantine(file, accepting: suggestion) } catch { failed(ProjectStore.message(error)) }
      }
    }
    .padding(.vertical, 4)
  }
}

/// The Quarantine section of the test's details: whether it's in quarantine, until when, why,
/// and the way out; a test that keeps failing and passing by itself is offered quarantine here.
struct QuarantineSection: View {
  @Environment(ProjectStore.self) private var project
  let file: String
  @State private var editing = false
  @State private var draft = QuarantineDraft()
  @State private var failure: String?

  var body: some View {
    Section {
      if let entry = project.quarantine(of: file) {
        LabeledContent {
          HStack {
            Button("Change…") { edit(entry) }
            Button("Release from Quarantine") {
              do { try project.releaseFromQuarantine(file) } catch { failure = ProjectStore.message(error) }
            }
          }
        } label: {
          Label { Text(QuarantineText.describe(entry)) } icon: { QuarantineMark(entry: entry) }
        }
      } else {
        if let suggestion = project.quarantineSuggestion(for: file) {
          QuarantineSuggestionView(file: file, suggestion: suggestion) { failure = $0 }
        }
        LabeledContent {
          Button("Quarantine…") { edit(nil) }
        } label: {
          Text("Not in quarantine")
        }
      }
      if editing {
        QuarantineFields(draft: $draft)
        QuarantineFields.footnote(draft)
          .font(.callout)
          .foregroundStyle(.secondary)
        HStack {
          Spacer()
          Button("Cancel", role: .cancel) { editing = false }
          Button(project.quarantine(of: file) == nil ? "Put in Quarantine" : "Save") { save() }
            .keyboardShortcut(.defaultAction)
        }
      }
    } header: {
      Text("Quarantine")
    } footer: {
      Text(QuarantineText.explanation)
    }
    .alert(
      "Can’t change the quarantine", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } }),
      presenting: failure
    ) { _ in
      Button("OK") {}
    } message: { Text($0) }
  }

  private func edit(_ entry: Quarantined?) {
    draft = QuarantineDraft(entry)
    editing = true
  }

  private func save() {
    do {
      try project.quarantine(file, reason: draft.reason, until: draft.lastDay)
      editing = false
    } catch {
      failure = ProjectStore.message(error)
    }
  }
}
