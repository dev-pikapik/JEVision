import JEVisionCore
import SwiftUI

/// The schedules of the project: what runs when, when it runs next and how it went last.
struct SchedulesSheet: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  /// The schedule being added or changed.
  @State private var editing: ScheduleDraft?
  @State private var failure: String?

  private var schedules: ProjectSchedules { project.schedules }

  var body: some View {
    Group {
      if let draft = editing {
        ScheduleEditor(draft: draft, others: schedules.all.filter { $0.name != draft.originalName }) { saved in
          save(replacing: draft.originalName, with: saved)
        } cancel: {
          editing = nil
        }
        .id(draft.originalName)
      } else {
        list
      }
    }
    .padding(20)
    .frame(width: 520)
    .alert(
      "Can’t save the schedules", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } })
    ) {
      Button("OK") {}
    } message: {
      Text(failure ?? "")
    }
  }

  private var list: some View {
    VStack(alignment: .leading, spacing: 16) {
      VStack(alignment: .leading, spacing: 4) {
        Text("Schedules")
          .font(.title2.bold())
          .accessibilityAddTraits(.isHeader)
        Text("Tests run on their own at the times you choose, on this Mac.")
          .foregroundStyle(.secondary)
      }
      if let problem = schedules.problem {
        Label(problem, systemImage: "exclamationmark.triangle.fill")
          .foregroundStyle(.orange)
      }
      if schedules.all.isEmpty {
        ContentUnavailableView(
          "No schedules yet", systemImage: "calendar.badge.clock",
          description: Text("Run all tests every night, a folder every weekday morning, or a few tests every few hours."))
          .frame(minHeight: 180)
      } else {
        ScrollView {
          VStack(spacing: 0) {
            ForEach(Array(schedules.all.enumerated()), id: \.element.name) { index, schedule in
              if index > 0 { Divider() }
              ScheduleRow(schedule: schedule, lastRun: schedules.lastRun(of: schedule.name)) {
                editing = ScheduleDraft(schedule)
              } delete: {
                save(replacing: schedule.name, with: nil)
              }
            }
          }
        }
        .frame(minHeight: 120, maxHeight: 360)
        .fixedSize(horizontal: false, vertical: true)
        status
      }
      HStack {
        Button("Add Schedule…") { editing = ScheduleDraft(name: newName, environment: nil) }
          .disabled(schedules.problem != nil)
        Spacer()
        Button("Done") { dismiss() }
          .keyboardShortcut(.defaultAction)
      }
    }
  }

  /// Whether the schedules run on this Mac, and what they need from it.
  @ViewBuilder private var status: some View {
    let tool = CommandLineTool.bundled
    if let reason = tool.unavailableReason {
      Label { Text(reason) } icon: { Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange) }
    } else if schedules.isOn {
      Label(
        "They run while you’re logged in and the Mac is awake. A run is skipped while the screen is locked; for runs nobody watches, turn off the automatic screen lock.",
        systemImage: "checkmark.circle.fill"
      )
      .foregroundStyle(.secondary)
      .font(.callout)
    } else {
      HStack(alignment: .firstTextBaseline) {
        Label("These schedules don’t run on this Mac yet.", systemImage: "pause.circle")
        Spacer()
        Button("Turn On") {
          do { try schedules.turnOn() } catch { failure = error.localizedDescription }
        }
        .disabled(schedules.problem != nil)
      }
    }
  }

  /// “Weekday Mornings”, as a new schedule runs at 9:00 AM on weekdays: a screen locked at night
  /// would skip a nightly run. Then “Weekday Mornings 2” and so on, whichever is free.
  private var newName: String {
    Self.freeName(String(localized: "Weekday Mornings"), taken: schedules.all.map(\.name))
  }

  /// `base`, or else `base 2`, `base 3` and so on: the first that isn't in `taken`, ignoring case.
  nonisolated static func freeName(_ base: String, taken: [String]) -> String {
    let taken = Set(taken.map { $0.lowercased() })
    guard taken.contains(base.lowercased()) else { return base }
    return (2...).lazy.map { "\(base) \($0)" }.first { !taken.contains($0.lowercased()) }!
  }

  /// Puts `saved` in place of the schedule named `name`, or at the end; without `saved`,
  /// deletes that schedule.
  private func save(replacing name: String?, with saved: ProjectSchedule?) {
    var all = schedules.all
    if let index = all.firstIndex(where: { $0.name == name }) {
      if let saved { all[index] = saved } else { all.remove(at: index) }
    } else if let saved {
      all.append(saved)
    }
    do {
      try schedules.save(all)
      editing = nil
    } catch {
      failure = error.localizedDescription
    }
  }
}

private struct ScheduleRow: View {
  let schedule: ProjectSchedule
  let lastRun: ScheduledRun?
  let edit: () -> Void
  let delete: () -> Void
  @State private var confirmsDelete = false

  var body: some View {
    HStack(alignment: .top, spacing: 12) {
      Image(systemName: "calendar.badge.clock")
        .font(.title2)
        .foregroundStyle(.tint)
        .accessibilityHidden(true)
      VStack(alignment: .leading, spacing: 3) {
        Text(verbatim: schedule.name).font(.headline)
        Text(verbatim: ScheduleText.summary(schedule))
        Text(verbatim: ScheduleText.tests(schedule.tests)).foregroundStyle(.secondary)
        if let next = ProjectSchedules.nextRun(of: schedule) {
          Text("Next run \(ScheduleText.moment(next))").foregroundStyle(.secondary)
        }
        if let lastRun {
          Label {
            Text(verbatim: "\(ScheduleText.outcome(lastRun)) · \(ScheduleText.moment(lastRun.startedAt))")
          } icon: {
            Image(systemName: lastRun.passed ? "checkmark.circle.fill" : "xmark.circle.fill")
              .foregroundStyle(lastRun.passed ? .green : .red)
          }
          .foregroundStyle(.secondary)
        }
      }
      .font(.callout)
      Spacer(minLength: 0)
      Button("Edit…", action: edit)
      Button {
        confirmsDelete = true
      } label: {
        Image(systemName: "trash")
      }
      .help(Text("Delete Schedule"))
      .accessibilityLabel(Text("Delete Schedule"))
    }
    .padding(.vertical, 10)
    .contentShape(.rect)
    .onTapGesture(count: 2, perform: edit)
    .confirmationDialog(
      "Delete the schedule “\(schedule.name)”?", isPresented: $confirmsDelete
    ) {
      Button("Delete", role: .destructive, action: delete)
    } message: {
      Text("Its tests stay; they just stop running on their own.")
    }
  }
}
