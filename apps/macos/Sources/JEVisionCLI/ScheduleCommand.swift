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
      Console.standardError.line("jevision: there is no folder “\(project)”")
      return .failure
    }
    var status = ExitStatus.ok
    // Runs that came due while others ran are run next.
    while true {
      let due: [ProjectSchedule]
      do {
        due = try beginScheduleCheck(project: root, now: LocalMinute.of(now()))
      } catch {
        Console.standardError.line("jevision: \(error.localizedDescription)")
        return .failure
      }
      if due.isEmpty { return status }
      for schedule in due {
        status = max(status, run(schedule, in: root))
      }
    }
  }

  private func run(_ schedule: ProjectSchedule, in root: String) -> ExitStatus {
    let started = now()
    let outcome: ScheduledOutcome
    var status: ExitStatus
    if let reason = blocker() {
      progress.line("Skipped “\(schedule.name)”: \(Self.describe(reason))")
      outcome = .skipped(reason: reason)
      status = .problems
    } else {
      progress.line("Schedule “\(schedule.name)”")
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
        let message = error.message.hasPrefix("jevision: ") ? String(error.message.dropFirst(10)) : error.message
        Console.standardError.line("jevision: \(message)")
        outcome = .error(message: message)
        status = .failure
      }
    }
    let run = ScheduledRun(
      schedule: schedule.name, startedAtMs: Int64(started.timeIntervalSince1970 * 1000),
      durationMs: UInt64(max(0, now().timeIntervalSince(started)) * 1000), outcome: outcome)
    do {
      try recordScheduledRun(project: root, run: run)
    } catch {
      Console.standardError.line("jevision: the schedule history couldn’t be saved: \(error.localizedDescription)")
      status = .failure
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

  static func describe(_ reason: SkipReason) -> String {
    switch reason {
    case .screenLocked: "the screen is locked."
    case .noUser: "nobody is logged in with a screen."
    case .notAllowed: "macOS hasn’t allowed jevision to record the screen and control the computer."
    }
  }
}
