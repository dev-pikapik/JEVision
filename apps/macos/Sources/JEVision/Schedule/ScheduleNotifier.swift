import AppKit
import JEVisionCore
import JEVisionHands
import UserNotifications

/// Tells the user when a scheduled run fails or is skipped. Runs happen in `jevision`, started
/// by launchd, which can't post notifications; the app looks at the history of every project
/// with schedules at launch and every minute, and tells about the runs it hasn't told about.
@MainActor
final class ScheduleNotifier {
  /// Per project folder, the start of the newest scheduled run already looked at, in ms.
  static let seenKey = "scheduleNotifiedUpTo"

  private let agents: ScheduleAgents
  private let defaults: UserDefaults
  private var timer: Timer?

  init(agents: ScheduleAgents = ScheduleAgents(), defaults: UserDefaults = Profile.current.defaults) {
    self.agents = agents
    self.defaults = defaults
  }

  func start() {
    check()
    timer = Timer.scheduledTimer(withTimeInterval: 60, repeats: true) { [weak self] _ in
      MainActor.assumeIsolated { self?.check() }
    }
  }

  /// Posts a notification for every new run of a project with schedules that didn't pass.
  func check() {
    for project in agents.projects() {
      for run in Self.unseen(project, defaults: defaults) where !run.passed {
        Self.post(run, project: project)
      }
    }
  }

  /// Runs of `project` that weren't looked at yet, oldest first; they count as looked at now.
  /// A project seen for the first time starts with what it has.
  static func unseen(_ project: String, defaults: UserDefaults) -> [ScheduledRun] {
    let runs = (try? scheduledRuns(project: project)) ?? []
    var seen = defaults.dictionary(forKey: seenKey) as? [String: Int64] ?? [:]
    let last = seen[project]
    seen[project] = max(last ?? .min, runs.first?.startedAtMs ?? 0)
    defaults.set(seen, forKey: seenKey)
    guard let last else { return [] }
    return runs.filter { $0.startedAtMs > last }.reversed()
  }

  /// From now on, every run of `project` is told about.
  static func startWatching(_ project: String, defaults: UserDefaults) {
    var seen = defaults.dictionary(forKey: seenKey) as? [String: Int64] ?? [:]
    guard seen[project] == nil else { return }
    seen[project] = Int64(Date.now.timeIntervalSince1970 * 1000)
    defaults.set(seen, forKey: seenKey)
  }

  /// Notifications work only in the app itself, not in tests or `swift run`. A separate profile
  /// runs no schedules, and the permission to notify is the person's, so it doesn't ask either.
  static var canNotify: Bool { Bundle.main.bundleIdentifier == AISettings.appDomain && !Profile.current.isolated }

  static func askPermission() {
    guard canNotify else { return }
    UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in }
  }

  private static func post(_ run: ScheduledRun, project: String) {
    guard canNotify else { return }
    let content = UNMutableNotificationContent()
    content.title = ScheduleText.notificationTitle(run)
    content.body = [ScheduleText.outcome(run), FileManager.default.displayName(atPath: project)].joined(separator: " · ")
    content.sound = .default
    let request = UNNotificationRequest(identifier: "\(project)#\(run.startedAtMs)", content: content, trigger: nil)
    UNUserNotificationCenter.current().add(request)
  }
}

extension ScheduledRun {
  var passed: Bool {
    if case .finished(_, let failed) = outcome { failed == 0 } else { false }
  }

  var startedAt: Date { Date(timeIntervalSince1970: Double(startedAtMs) / 1000) }
}
