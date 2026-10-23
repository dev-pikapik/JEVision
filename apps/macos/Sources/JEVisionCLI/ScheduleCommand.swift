import CoreGraphics
import Foundation
import JEVisionCore
import JEVisionHands

/// `jevision schedule run`: runs the schedules of a project that are due, as launchd starts it
/// at their times, and records what came of each one.
struct ScheduleCommand {
  let project: String
  var progress = Console.standardOutput
  var now: () -> Date = Date.init
  /// Why tests can't run on this Mac right now; `nil` when they can.
  var blocker: () -> SkipReason? = Self.blocker
  var runTests: (RunRequest) throws(UsageError) -> [TestResult] = { request throws(UsageError) in
    try RunCommand(request: request).runTests()
  }

  func execute() -> ExitStatus {
    guard let root = realpath(project, nil).map({ path in
      defer { free(path) }
      return String(cString: path)
    }), isFolder(root) else {
      Console.standardError.line("jevision: " + words("cli.error.noFolder", project))
      return .usage
    }
    var status = ExitStatus.ok
    // Runs that came due while others ran are run next.
    while true {
      let due: [ProjectSchedule]
      do {
        due = try beginScheduleCheck(project: root, now: LocalMinute.of(now()))
      } catch {
        Console.standardError.line("jevision: \(error.localizedDescription)")
        return Self.status(error, in: root)
      }
      if due.isEmpty { return status }
      for schedule in due {
        status = max(status, run(schedule, in: root))
      }
    }
  }

  /// How an error of the schedule file or of the history of the runs ends the command: `.needsNewer`
  /// when a newer JEVision saved the file, `.io` for any other.
  static func status(_ error: Error, in root: String) -> ExitStatus {
    let file: String
    switch error as? ScheduleError {
    case .Invalid(let code, _) where code == "project.format.tooNew": return .needsNewer
    case .Invalid(_, let params): file = params["file"] ?? scheduleFileName()
    case .Unreadable(let name, _), .Unwritable(let name, _): file = name
    case nil: file = scheduleFileName()
    }
    return .unreadable((root as NSString).appendingPathComponent(file))
  }

  private func run(_ schedule: ProjectSchedule, in root: String) -> ExitStatus {
    let started = now()
    let outcome: ScheduledOutcome
    var status: ExitStatus
    if let reason = blocker() {
      progress.line(Self.skipped(schedule.name, reason))
      outcome = .skipped(reason: reason)
      status = .notReady
    } else {
      progress.line(words("cli.schedule.started", schedule.name))
      var request = RunRequest(paths: Self.paths(of: schedule.tests, in: root), project: root)
      request.environment = schedule.environment
      request.video = schedule.video
      request.schedule = schedule.name
      do {
        let results = try runTests(request)
        let passed = results.count { $0.verdict == "passed" }
        outcome = .finished(passed: UInt32(passed), failed: UInt32(results.count - passed))
        status = results.map(\.status).max() ?? .ok
      } catch {
        Console.standardError.line("jevision: \(error.reason)")
        outcome = .error(message: error.reason)
        status = error.status
      }
    }
    let run = ScheduledRun(
      schedule: schedule.name, startedAtMs: Int64(started.timeIntervalSince1970 * 1000),
      durationMs: UInt64(max(0, now().timeIntervalSince(started)) * 1000), outcome: outcome)
    do {
      try recordScheduledRun(project: root, run: run)
    } catch {
      Console.standardError.line("jevision: " + words("cli.schedule.historyNotSaved", error.localizedDescription))
      status = max(status, Self.status(error, in: root))
    }
    return status
  }

  static func paths(of tests: ScheduledTests, in root: String) -> [String] {
    switch tests {
    case .all: [root]
    case .folder(let path): [(root as NSString).appendingPathComponent(path)]
    case .files(let paths): paths.map { (root as NSString).appendingPathComponent($0) }
    }
  }

  /// Checked before every schedule: the screen may lock while an earlier one runs.
  static func blocker() -> SkipReason? {
    guard CGSessionCopyCurrentDictionary() != nil else { return .noUser }
    if MacHands.screenIsLocked { return .screenLocked }
    return Readiness.problem() == nil ? nil : .notAllowed
  }

  /// The line for the schedule `name` that didn't run.
  static func skipped(_ name: String, _ reason: SkipReason) -> String {
    switch reason {
    case .screenLocked: words("cli.schedule.skippedLocked", name)
    case .noUser: words("cli.schedule.skippedNoUser", name)
    case .notAllowed: words("cli.schedule.skippedNotAllowed", name)
    }
  }
}
