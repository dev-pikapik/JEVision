import Foundation

// Texts match the Mac app's word for word, so both read a failure the same way.

/// Run results in words. The catalog keys are the core's failure codes.
enum RunText {
  static func failure(_ failure: Failure) -> String {
    let p = { (name: String) in failure.params[name] ?? "" }
    let block = p("block")
    let app = p("app")
    switch failure.code {
    case "run.test.unreadable":
      return String(localized: "run.test.unreadable", defaultValue: "Can’t read the test file \(p("test")): \(p("reason")).")
    case "run.test.invalid":
      return String(localized: "run.test.invalid", defaultValue: "The test has problems to fix first (\(p("errors"))). They’re marked on the blocks.")
    case "run.start.unknownBlock":
      return String(localized: "run.start.unknownBlock", defaultValue: "There’s no block “\(block)” to start from.")
    case "run.secret.missing":
      return String(localized: "run.secret.missing", defaultValue: "The secret \(p("name")) isn’t set. Set it in Test details.")
    case "run.path.outsideProject":
      return String(localized: "run.path.outsideProject", defaultValue: "Block “\(block)”: \(p("path")) is outside the project folder.")
    case "run.file.missing":
      return String(localized: "run.file.missing", defaultValue: "Block “\(block)”: the file \(p("file")) doesn’t exist.")
    case "run.file.copyFailed":
      return String(localized: "run.file.copyFailed", defaultValue: "Block “\(block)”: can’t copy \(p("file")): \(p("reason")).")
    case "run.target.notFound":
      return String(localized: "run.target.notFound", defaultValue: "Block “\(block)”: \(p("image")) wasn’t found on the screen.")
    case "run.reference.missing":
      return String(localized: "run.reference.missing", defaultValue: "Block “\(block)”: the reference image \(p("image")) doesn’t exist.")
    case "run.reference.unreadable":
      return String(localized: "run.reference.unreadable", defaultValue: "Block “\(block)”: can’t read the image \(p("image")): \(p("reason")).")
    case "run.screenshot.unreadable":
      return String(localized: "run.screenshot.unreadable", defaultValue: "Block “\(block)”: can’t read the screenshot: \(p("reason")).")
    case "run.compare.shapeMismatch":
      return String(
        localized: "run.compare.shapeMismatch",
        defaultValue: "Block “\(block)”: the window is \(p("actual")), but the reference \(p("image")) is \(p("expected")).")
    case "run.compare.meaningUnavailable":
      return String(
        localized: "run.compare.meaningUnavailable",
        defaultValue: "Block “\(block)”: comparing by meaning needs an AI model. Choose one in Settings → AI.")
    case "run.check.meaningMismatch":
      return String(
        localized: "run.check.meaningMismatch",
        defaultValue: "Block “\(block)”: the screen doesn’t show what’s expected. \(p("provider")): \(p("reason"))")
    case "run.vision.cloudBlocked":
      return String(
        localized: "run.vision.cloudBlocked",
        defaultValue: "Block “\(block)”: only models on this Mac are allowed, and \(p("provider")) works in the cloud. Choose a local model or turn off “Only models on this Mac” in Settings → AI.")
    case let code where code.hasPrefix("run.vision."):
      return String(localized: "Block “\(block)”: \(VisionText.failure(failure))")
    case "run.click.noChange":
      return String(localized: "run.click.noChange", defaultValue: "Block “\(block)”: the screen didn’t change within \(p("seconds")) s after the click.")
    case "run.waitFor.timeout" where p("target") == "screenStable":
      return String(localized: "run.waitFor.timeout.screenStable", defaultValue: "Block “\(block)”: the screen didn’t settle within \(p("seconds")) s.")
    case "run.waitFor.timeout":
      return String(localized: "run.waitFor.timeout", defaultValue: "Block “\(block)”: waited \(p("seconds")) s for \(p("target")), but it didn’t happen.")
    case "run.check.mismatch":
      return String(
        localized: "run.check.mismatch",
        defaultValue: "Block “\(block)”: the screen matches \(p("image")) by \(p("similarity")), and at least \(p("threshold")) is needed.")
    case "run.data.unreadable":
      return String(localized: "run.data.unreadable", defaultValue: "Block “\(block)”: can’t read the table \(p("file")): \(p("reason")).")
    case "run.data.unknownColumn":
      return String(localized: "run.data.unknownColumn", defaultValue: "Block “\(block)”: there’s no table column “\(p("column"))” here.")
    case "run.runTest.recursion":
      return String(
        localized: "run.runTest.recursion",
        defaultValue: "Block “\(block)”: the test \(p("test")) is already running, and a test can’t run itself.")
    case "run.io.writeFailed":
      return String(localized: "run.io.writeFailed", defaultValue: "Can’t save \(p("path")): \(p("reason")).")
    case "run.hands.permissionDenied":
      let name = permissionTitle(p("permission"))
      return String(localized: "run.hands.permissionDenied", defaultValue: "JEVision isn’t allowed to use \(name). Turn it on in Settings → Permissions.")
    case "run.hands.appNotFound":
      return String(localized: "run.hands.appNotFound", defaultValue: "Block “\(block)”: \(app) isn’t installed on this Mac.")
    case "run.hands.noWindow":
      return String(localized: "run.hands.noWindow", defaultValue: "Block “\(block)”: \(app) has no open window.")
    case "run.hands.cannotOpen":
      return String(localized: "run.hands.cannotOpen", defaultValue: "Block “\(block)”: can’t open \(p("file")).")
    case "run.hands.screenLocked":
      return String(localized: "run.hands.screenLocked", defaultValue: "Block “\(block)”: the screen is locked. Unlock the Mac and run the test again.")
    case "run.hands.failed":
      return String(localized: "run.hands.failed", defaultValue: "Block “\(block)”: \(p("reason"))")
    default:
      return block.isEmpty
        ? String(localized: "The test failed. The report on the Mac has the details.")
        : String(localized: "Block “\(block)” failed. The report on the Mac has the details.")
    }
  }

  private static func permissionTitle(_ permission: String) -> String {
    switch permission {
    case "screenRecording": String(localized: "Screen Recording")
    case "accessibility": String(localized: "Accessibility")
    default: permission
    }
  }

  /// Which way an `if` step went, named as the Mac's canvas names the ways out:
  /// Took “Condition 2: Inbox is empty”, Took “Otherwise”. `nil` for other blocks.
  static func branch(_ step: RunReport.Step) -> String? {
    step.branch.flatMap { branch($0, branches: step.branches, condition: step.branchCondition) }
  }

  /// `branch` is the way out taken, `branches` how many the `if` had and `condition` what the
  /// way taken looks for.
  static func branch(_ branch: String, branches: Int?, condition: String?) -> String? {
    let index = branch.hasPrefix("elseIf") ? Int(branch.dropFirst(6)).flatMap { $0 > 0 ? $0 - 1 : nil } : nil
    let plain = branches.map { $0 <= 2 } ?? (index == nil)
    let name: String
    switch branch {
    case "then": name = plain ? String(localized: "Yes") : String(localized: "Condition \(1)")
    case "else": name = plain ? String(localized: "No") : String(localized: "Otherwise")
    default:
      guard let index else { return nil }
      name = String(localized: "Condition \(index + 2)")
    }
    guard let condition else { return String(localized: "Took “\(name)”") }
    return String(localized: "Took “\(name): \(condition)”")
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
    let p = { (name: String) in failure.params[name] ?? "" }
    let provider = p("provider")
    switch failure.code {
    case "run.vision.noModel":
      return String(localized: "run.vision.noModel", defaultValue: "No model is chosen for \(provider). Choose one in Settings → AI.")
    case "run.vision.noKey":
      return String(localized: "run.vision.noKey", defaultValue: "The API key for \(provider) isn’t set. Set it in Settings → AI.")
    case "run.vision.badAddress":
      return String(
        localized: "run.vision.badAddress", defaultValue: "“\(p("address"))” isn’t a web address. It should start with http:// or https://.")
    case "run.vision.unreachable":
      return String(
        localized: "run.vision.unreachable",
        defaultValue: "Can’t reach \(provider) at \(p("address")). Check that it’s running and the Mac is online.")
    case "run.vision.timeout":
      return String(localized: "run.vision.timeout", defaultValue: "\(provider) didn’t answer within \(p("seconds")) s.")
    case "run.vision.badKey":
      return String(localized: "run.vision.badKey", defaultValue: "\(provider) didn’t accept the API key. Set it again in Settings → AI.")
    case "run.vision.unknownModel":
      return String(localized: "run.vision.unknownModel", defaultValue: "\(provider) has no model “\(p("model"))”. Choose another one in Settings → AI.")
    case "run.vision.rateLimited":
      return String(
        localized: "run.vision.rateLimited",
        defaultValue: "\(provider) has had too many requests, or the account’s limit is reached. Try again later. \(p("message"))")
    case "run.vision.serviceError":
      return String(localized: "run.vision.serviceError", defaultValue: "\(provider) answered with error \(p("status")): \(p("message"))")
    case "run.vision.refused":
      return String(localized: "run.vision.refused", defaultValue: "\(provider) declined to judge the screen: \(p("message"))")
    case "run.vision.badAnswer":
      return String(localized: "run.vision.badAnswer", defaultValue: "\(provider) gave an answer JEVision can’t read: \(p("answer"))")
    default:
      return String(localized: "The AI model couldn’t judge the screen. The report on the Mac has the details.")
    }
  }
}
