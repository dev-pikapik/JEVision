import Foundation
import JEVisionCore

/// Run results in words. The catalog keys are the core's failure codes.
@MainActor
enum RunText {
  static func failure(_ failure: Failure) -> String {
    let p = { (name: String) in failure.params[name] ?? "" }
    let block = p("block")
    let app = AppCatalog.shared.name(of: p("app"))
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
        defaultValue: "Block “\(block)”: comparing by meaning needs a vision model, and none is set up.")
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
      let name = permission(of: failure)?.title ?? p("permission")
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
      return describeFailure(failure: failure)
    }
  }

  /// The permission a failure asks for, if it is about one.
  static func permission(of failure: Failure) -> Permission? {
    guard failure.code == "run.hands.permissionDenied" else { return nil }
    switch failure.params["permission"] {
    case "screenRecording": return .screenRecording
    case "accessibility": return .accessibility
    default: return nil
    }
  }

  /// How the run ended, in one sentence.
  static func outcome(_ report: RunReport) -> String {
    switch report.verdict {
    case .cancelled:
      return String(localized: "You stopped the test.")
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
