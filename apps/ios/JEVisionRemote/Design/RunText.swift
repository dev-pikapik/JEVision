import Foundation
import JEVisionMobile

// The wording of failures and of which way an `if` went comes from the Rust core, from the
// catalog the Mac app shares, so the phone words them exactly as the Mac does.

/// Run results in words.
enum RunText {
  static func failure(_ failure: Failure) -> String {
    describeFailure(failure: JEVisionMobile.Failure(code: failure.code, params: failure.params))
  }

  /// Who has the screen, for a run that waits for its turn: Screen busy: JEVision, test Sign in, since 12:03.
  static func screenBusy(_ holder: ScreenHolder?) -> String {
    guard let holder else { return localizedText(key: "screen.busy.unknown", args: []) }
    let since = holder.since.formatted(date: .omitted, time: .shortened)
    return localizedText(key: "screen.busy", args: [.text(value: holder.process), .text(value: holder.test), .text(value: since)])
  }

  /// Which way an `if` step went, named as the Mac's canvas names the ways out:
  /// Took “Condition 2: Inbox is empty”, Took “Otherwise”. `nil` for other blocks.
  static func branch(_ step: RunReport.Step) -> String? {
    step.branch.flatMap { branch($0, branches: step.branches, condition: step.branchCondition) }
  }

  /// `branch` is the way out taken, `branches` how many the `if` had and `condition` what the
  /// way taken looks for.
  static func branch(_ branch: String, branches: Int?, condition: String?) -> String? {
    branchText(branch: branch, branches: branches.map { UInt32($0) }, condition: condition)
  }

  /// How the run ended, in one sentence.
  static func outcome(_ report: RunReport) -> String {
    switch report.verdict {
    case .cancelled:
      return String(localized: "You stopped the test.")
    case .blocked:
      guard let name = report.blockedBy else {
        return String(localized: "The test didn’t run: a test it depends on didn’t pass.")
      }
      return String(localized: "Blocked: \(name) didn’t pass. The test didn’t run.")
    case .unknown:
      return String(localized: "The Mac reported a result this version of the app doesn’t know. Update the app to see it.")
    case .failed:
      return report.attempts.last?.failure.map(failure) ?? String(localized: "The test failed.")
    case .passed:
      let time = Duration.milliseconds(report.durationMs).formatted(.units(allowed: [.minutes, .seconds], width: .abbreviated))
      if report.flaky {
        let tries = report.attempts.count
        return String(localized: "Passed on try \(tries), so the test is flaky: it failed before that.")
      }
      if !report.baselinesCreated.isEmpty {
        return String(localized: "Passed in \(time). New reference images were saved: check them in the report.")
      }
      return String(localized: "Passed in \(time).")
    }
  }
}

/// Problems with an AI model in words: `run.vision.*` codes, without the block they happened in.
enum VisionText {
  static func failure(_ failure: Failure) -> String {
    describeVisionFailure(failure: JEVisionMobile.Failure(code: failure.code, params: failure.params))
  }
}
