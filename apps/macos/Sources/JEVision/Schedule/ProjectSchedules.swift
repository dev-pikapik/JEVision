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
  /// Scheduled runs on this Mac, newest first.
  private(set) var runs: [ScheduledRun] = []
  /// Why the schedule file can't be read.
  private(set) var problem: String?
  /// macOS has the job that starts the schedules, as they are now.
  private(set) var isOn = false
  /// The Schedules sheet is shown.
  var isShowingSheet = false

  @ObservationIgnored private var project: URL?
  @ObservationIgnored let agents: ScheduleAgents
  @ObservationIgnored private let defaults: UserDefaults

  init(agents: ScheduleAgents = ScheduleAgents(), defaults: UserDefaults = .standard) {
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
    guard let project else { return }
    let path = project.path(percentEncoded: false)
    do {
      all = try projectSchedules(project: path)
      problem = nil
    } catch ScheduleError.Unreadable(let file, let reason) {
      problem = String(localized: "\(file) can’t be read: \(reason)")
    } catch {
      problem = error.localizedDescription
    }
    runs = (try? scheduledRuns(project: path)) ?? []
    var state = agents.state(of: project, schedules: all)
    if problem == nil, state.installed, !state.current {
      try? agents.install(project, schedules: all)
      state = agents.state(of: project, schedules: all)
    }
    isOn = !all.isEmpty && state.current
  }

  /// Saves `schedules` to the schedule file and has macOS run them on this Mac.
  func save(_ schedules: [ProjectSchedule]) throws {
    guard let project else { return }
    try saveProjectSchedules(project: project.path(percentEncoded: false), schedules: schedules)
    defer { reload() }
    try turnOn(schedules)
  }

  /// Has macOS run the schedules of the file on this Mac.
  func turnOn() throws {
    defer { reload() }
    try turnOn(all)
  }

  private func turnOn(_ schedules: [ProjectSchedule]) throws {
    guard let project else { return }
    try agents.install(project, schedules: schedules)
    guard !schedules.isEmpty else { return }
    ScheduleNotifier.startWatching(ScheduleAgents.path(project), defaults: defaults)
    ScheduleNotifier.askPermission()
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
