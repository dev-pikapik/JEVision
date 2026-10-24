import JEVisionCore
import SwiftUI

/// The schedules of the project: what runs when, when it runs next and how it went last.
struct SchedulesSheet: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  /// The sheet's own Undo: changes are saved at once, while the sheet stays open.
  @Environment(\.undoManager) private var undoManager
  /// The schedule being added or changed.
  @State private var editing: ScheduleDraft?
  @State private var failure: String?
  /// Why macOS didn't take up the schedules, after saving them or on Turn On.
  @State private var turnOnFailure: TurnOnFailure?
  /// What changed in the file while a schedule was being edited, when that stopped saving it.
  @State private var conflict: Conflict?

  private struct TurnOnFailure {
    /// The schedules were saved just before.
    let saved: Bool
    let reason: String
    /// What macOS said, in its own words.
    let detail: String?

    init(saved: Bool, _ error: any Error) {
      self.saved = saved
      reason = error.localizedDescription
      detail = (error as? ScheduleAgents.Failure)?.detail
    }
  }

  private struct Conflict {
    let kind: ScheduleDraft.Conflict
    let draft: ScheduleDraft
    let saved: ProjectSchedule
  }

  private var schedules: ProjectSchedules { project.schedules }

  var body: some View {
    Group {
      if let draft = editing {
        ScheduleEditor(draft: draft, others: schedules.all.filter { $0.name != draft.originalName }) { saved in
          commit(draft, as: saved)
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
    .alert(
      turnOnFailureTitle,
      isPresented: Binding(get: { turnOnFailure != nil }, set: { if !$0 { turnOnFailure = nil } }),
      presenting: turnOnFailure
    ) { _ in
      Button("OK") {}
    } message: { failure in
      Text(verbatim: turnOnFailureMessage(failure))
    }
    .alert(
      conflictTitle, isPresented: Binding(get: { conflict != nil }, set: { if !$0 { conflict = nil } }),
      presenting: conflict
    ) { conflict in
      switch conflict.kind {
      case .changed:
        Button("Replace with Your Changes", role: .destructive) {
          commit(conflict.draft, as: conflict.saved, anyway: true)
        }
      case .removed:
        Button("Save It Again") { commit(conflict.draft, as: conflict.saved, anyway: true) }
      case .nameTaken: EmptyView()
      }
      Button("Discard Changes", role: .destructive) { editing = nil }
      Button("Keep Editing", role: .cancel) {}
    } message: { conflict in
      switch conflict.kind {
      case .changed:
        Text("It was changed after you started editing it. Saving would replace those changes with yours.")
      case .removed: Text("It was deleted after you started editing it. Saving brings it back with your changes.")
      case .nameTaken: Text("It was added after you started editing. Give this schedule another name.")
      }
    }
  }

  private var turnOnFailureTitle: Text {
    turnOnFailure?.saved == false
      ? Text("Can’t turn on the schedules") : Text("The schedules are saved, but they don’t run on this Mac")
  }

  private func turnOnFailureMessage(_ failure: TurnOnFailure) -> String {
    var lines = [failure.reason]
    if offersTurnOn { lines.append(String(localized: "To try again, click Turn On.")) }
    if let detail = failure.detail { lines.append(String(localized: "macOS said: “\(detail)”")) }
    return lines.joined(separator: "\n\n")
  }

  /// The list shows Turn On: there are schedules this copy can run, and they don't run yet.
  private var offersTurnOn: Bool {
    !schedules.all.isEmpty && !schedules.isOn && schedules.problem == nil
      && CommandLineTool.bundled.unavailableReason == nil
  }

  private var conflictTitle: Text {
    guard let conflict else { return Text(verbatim: "") }
    let name = conflict.draft.originalName ?? conflict.saved.name
    return switch conflict.kind {
    case .changed: Text("“\(name)” was changed somewhere else")
    case .removed: Text("“\(name)” was deleted somewhere else")
    case .nameTaken: Text("Another schedule is now called “\(conflict.saved.name)”")
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
      if !schedules.warnings.isEmpty {
        ScrollView {
          VStack(alignment: .leading, spacing: 4) {
            ForEach(Array(schedules.warnings.enumerated()), id: \.offset) { _, issue in
              Label(describeIssue(issue: issue), systemImage: "exclamationmark.triangle")
                .font(.callout)
                .foregroundStyle(.secondary)
            }
          }
        }
        .frame(maxHeight: 100)
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
                save(replacing: schedule.name, with: nil, action: String(localized: "Delete Schedule"))
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
    if !schedules.agents.isAvailable {
      Label(
        "Schedules don’t run in a separate profile: macOS would start them for everyone who uses JEVision on this Mac.",
        systemImage: "pause.circle"
      )
      .foregroundStyle(.secondary)
      .font(.callout)
    } else if let reason = tool.unavailableReason {
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
          Task {
            do {
              try await schedules.turnOn()
            } catch {
              turnOnFailure = TurnOnFailure(saved: false, error)
            }
          }
        }
        .disabled(schedules.problem != nil || schedules.isChanging)
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

  /// Saves the schedule edited from `draft` as `saved`. When the file changed since in a way that
  /// saving would overwrite or clash with, the person chooses first, unless `anyway`.
  private func commit(_ draft: ScheduleDraft, as saved: ProjectSchedule, anyway: Bool = false) {
    schedules.reload()
    if !anyway, schedules.problem == nil, let kind = draft.conflict(saving: saved, in: schedules.all) {
      conflict = Conflict(kind: kind, draft: draft, saved: saved)
      return
    }
    let action = draft.original == nil ? String(localized: "Add Schedule") : String(localized: "Edit Schedule")
    save(replacing: draft.originalName, with: saved, action: action)
  }

  /// Puts `saved` in place of the schedule named `name`, or at the end; without `saved`,
  /// deletes that schedule. The change is made to what the file holds now, as one step for Undo
  /// named `action`, so changes made to the file somewhere else are kept.
  private func save(replacing name: String?, with saved: ProjectSchedule?, action: String) {
    schedules.reload()
    if let problem = schedules.problem {
      failure = problem
      return
    }
    var all = schedules.all
    var renaming: (oldName: String, newName: String)?
    if let index = all.firstIndex(where: { $0.name == name }) {
      if let saved {
        if all[index].name != saved.name { renaming = (all[index].name, saved.name) }
        all[index] = saved
      } else { all.remove(at: index) }
    } else if let saved {
      all.append(saved)
    }
    do {
      let turningOn = try schedules.save(all, renaming: renaming, action: action, undoManager: undoManager)
      editing = nil
      Task {
        do {
          try await turningOn.value
        } catch {
          turnOnFailure = TurnOnFailure(saved: true, error)
        }
      }
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
