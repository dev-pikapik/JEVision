import Foundation
import JEVisionCore
import JEVisionHands
import Observation

/// The schedules of the open project, what came of their runs on this Mac, and whether they
/// run here. Schedules live in the project's schedule file; macOS starts them through a
/// launchd job the app installs once someone saves schedules on this Mac.
@MainActor
@Observable
final class ProjectSchedules {
  private(set) var all: [ProjectSchedule] = []
  private(set) var warnings: [Issue] = []
  /// Scheduled runs on this Mac, newest first.
  private(set) var runs: [ScheduledRun] = []
  /// Why the schedule file or the history of its runs can't be read; the schedules don't run
  /// on this Mac until it can.
  private(set) var problem: String?
  /// macOS has the job that starts the schedules, as they are now.
  private(set) var isOn = false
  /// macOS is being asked to take up a change of the schedules.
  var isChanging: Bool { changes > 0 }
  /// The Schedules sheet is shown.
  var isShowingSheet = false

  @ObservationIgnored private var project: URL?
  @ObservationIgnored let agents: ScheduleAgents
  @ObservationIgnored private let defaults: UserDefaults
  /// The changes asked of macOS that haven't ended, and the last of them.
  private var changes = 0
  @ObservationIgnored private var lastChange: Task<Result<Void, any Error>, Never>?

  init(agents: ScheduleAgents = ScheduleAgents(), defaults: UserDefaults = Profile.current.defaults) {
    self.agents = agents
    self.defaults = defaults
  }

  func open(_ url: URL) {
    project = url
    reload()
  }

  /// Reads the schedule file and the history again, after they changed. Schedules turned on
  /// on this Mac keep following the file, such as after a pull brings someone's changes.
  func reload() {
    read()
    follow(evenIfOff: false)
  }

  private func read() {
    guard let project else { return }
    let path = project.path(percentEncoded: false)
    do {
      all = try projectSchedules(project: path)
      warnings = try projectScheduleWarnings(project: path)
      problem = nil
    } catch {
      warnings = []
      problem = error.localizedDescription
    }
    do {
      runs = try scheduledRuns(project: path)
    } catch {
      runs = []
      problem = problem ?? error.localizedDescription
    }
  }

  /// Has macOS replace a job that no longer matches the file and, when `evenIfOff`, install one
  /// that isn't there. While a change is under way, it reads the file again when it ends.
  private func follow(evenIfOff: Bool) {
    guard let project else { return }
    guard changes == 0 else {
      if evenIfOff, problem == nil { change(to: all) }
      return
    }
    let state = agents.state(of: project, schedules: all)
    if problem == nil, !state.current, state.installed || evenIfOff {
      change(to: all)
    } else {
      isOn = !all.isEmpty && state.current
    }
  }

  /// Saves `schedules` to the schedule file and has macOS run them on this Mac, as one step of
  /// `undoManager` named `action`. Its Undo and Redo put the file back and have macOS run what
  /// the file holds then. Returns once the file is saved; macOS is asked in the background, and
  /// the task returned fails with the reason when the schedules don't run in the end.
  func save(
    _ schedules: [ProjectSchedule], renaming: (oldName: String, newName: String)? = nil,
    action: String, undoManager: UndoManager?,
    ask: @escaping FileUndo.Ask = FileUndo.askToReplace
  ) throws -> Task<Void, any Error> {
    guard let project else { return Task {} }
    let file = project.appending(path: scheduleFileName())
    try FileUndo.record(file, action: action, in: undoManager, ask: ask, then: { [weak self] in self?.followFile() }) {
      if let renaming {
        try saveProjectSchedulesRenaming(project: project.path(percentEncoded: false), schedules: schedules, from: renaming.oldName, to: renaming.newName)
      } else {
        try saveProjectSchedules(project: project.path(percentEncoded: false), schedules: schedules)
      }
    }
    let asked = change(to: schedules)
    read()
    return Task { try await settle(asked) }
  }

  /// Has macOS run what the schedule file holds after Undo or Redo put it back, as saving does.
  /// When that fails, the Schedules sheet shows they don't run on this Mac.
  private func followFile() {
    read()
    follow(evenIfOff: true)
  }

  /// Has macOS run the schedules of the file on this Mac; throws why, when they don't run.
  func turnOn() async throws {
    try await settle(change(to: all))
  }

  /// Has macOS run `schedules` on this Mac, away from the main thread and after the changes
  /// asked before. The last change to end reads the file again.
  @discardableResult
  private func change(to schedules: [ProjectSchedule]) -> Task<Result<Void, any Error>, Never> {
    guard let project else { return Task { .success(()) } }
    let (agents, defaults, previous) = (agents, defaults, lastChange)
    changes += 1
    let change = Task {
      _ = await previous?.value
      let result = await Task.detached { Result { try agents.install(project, schedules: schedules) } }.value
      if case .success = result, !schedules.isEmpty {
        ScheduleNotifier.startWatching(ScheduleAgents.path(project), defaults: defaults)
        ScheduleNotifier.askPermission()
      }
      changes -= 1
      if changes == 0 { reload() }
      return result
    }
    lastChange = change
    return change
  }

  /// Waits for `change` and the changes asked after it; throws why `change` failed when, after
  /// all of them, the schedules of the file don't run.
  private func settle(_ change: Task<Result<Void, any Error>, Never>) async throws {
    let result = await change.value
    var waited = change
    while let last = lastChange, last != waited {
      waited = last
      _ = await last.value
    }
    if case .failure(let error) = result, !all.isEmpty, !isOn { throw error }
  }

  /// The last run of the schedule named `name`.
  func lastRun(of name: String) -> ScheduledRun? {
    runs.first { $0.schedule == name }
  }

  /// When `schedule` runs next, after `now`.
  static func nextRun(of schedule: ProjectSchedule, after now: Date = .now) -> Date? {
    scheduleNextRun(schedule: schedule, after: LocalMinute.of(now)).map { LocalMinute.date($0) }
  }
}

/// The core words a schedule error in the app's language; with this, `localizedDescription` says
/// that sentence instead of the error's fields.
extension ScheduleError: @retroactive CustomStringConvertible {}
