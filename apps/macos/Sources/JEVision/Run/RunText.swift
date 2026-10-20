import Foundation
import JEVisionCore

/// Run results in words. The catalog keys are the core's failure codes.
@MainActor
enum RunText {
  static func failure(_ failure: Failure) -> String {
    let p = { (name: String) in failure.params[name] ?? "" }
    let block = p("block")
    let app = AppCatalog.shared.name(of: p("app"))
    let reason = p("reason").isEmpty ? "" : "\n" + p("reason")
    let code = { (name: String) in p(name) == "signal" ? String(localized: "none, the system stopped it") : p(name) }
    switch failure.code {
    case "run.test.unreadable":
      return String(localized: "run.test.unreadable", defaultValue: "Can’t read the test file \(p("test")): \(p("reason")).")
    case "run.test.invalid":
      return String(localized: "run.test.invalid", defaultValue: "The test has problems to fix first (\(p("errors"))). They’re marked on the blocks.")
    case "run.start.unknownBlock":
      return String(localized: "run.start.unknownBlock", defaultValue: "There’s no block “\(block)” to start from.")
    case "run.secret.missing":
      return String(localized: "run.secret.missing", defaultValue: "The secret \(p("name")) isn’t set. Set it in Test details.")
    case "run.input.missing":
      return String(localized: "run.input.missing", defaultValue: "Block “\(block)”: nothing gives the input \(p("name")). Run this block from a test that fills it in.")
    case "run.path.outsideProject":
      return String(localized: "run.path.outsideProject", defaultValue: "Block “\(block)”: \(p("path")) is outside the project folder.")
    case "run.file.missing":
      return String(localized: "run.file.missing", defaultValue: "Block “\(block)”: the file \(p("file")) doesn’t exist.")
    case "run.file.copyFailed":
      return String(localized: "run.file.copyFailed", defaultValue: "Block “\(block)”: can’t copy \(p("file")): \(p("reason")).")
    case "run.target.notFound":
      return String(localized: "run.target.notFound", defaultValue: "Block “\(block)”: \(p("image")) wasn’t found on the screen.")
    case "run.target.descriptionNotFound":
      return String(
        localized: "run.target.descriptionNotFound", defaultValue: "Block “\(block)”: “\(p("description"))” wasn’t found on the screen.")
    case "run.target.noLocator":
      return String(
        localized: "run.target.noLocator",
        defaultValue: "Block “\(block)”: finding an element by its description needs JEV (TypeSafe AI). Set it up in Settings → AI.")
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
    case "run.vision.cloudBlocked" where p("setting") == "project":
      return String(
        localized: "run.vision.cloudBlocked.project",
        defaultValue: "Block “\(block)”: this project allows only models on this Mac, and \(p("provider")) works in the cloud. Choose a local model, or turn off “Only models on this Mac for this project” in Settings → AI.")
    case "run.vision.cloudBlocked":
      return String(
        localized: "run.vision.cloudBlocked",
        defaultValue: "Block “\(block)”: only models on this Mac are allowed, and \(p("provider")) works in the cloud. Choose a local model or turn off “Only models on this Mac” in Settings → AI.")
    case let code where code.hasPrefix("run.vision."):
      return String(localized: "Block “\(block)”: \(VisionText.failure(failure))")
    case "run.click.noChange":
      return String(localized: "run.click.noChange", defaultValue: "Block “\(block)”: the screen didn’t change within \(p("seconds")) s after the click.")
    case "run.action.noChange":
      return String(localized: "run.action.noChange", defaultValue: "Block “\(block)”: the screen didn’t change within \(p("seconds")) s after this step.")
    case "run.waitFor.timeout" where p("target") == "screenStable":
      return String(localized: "run.waitFor.timeout.screenStable", defaultValue: "Block “\(block)”: the screen didn’t settle within \(p("seconds")) s.")
    case "run.waitFor.timeout":
      return String(localized: "run.waitFor.timeout", defaultValue: "Block “\(block)”: waited \(p("seconds")) s for \(p("target")), but it didn’t happen.")
    case "run.check.mismatch":
      return String(
        localized: "run.check.mismatch",
        defaultValue: "Block “\(block)”: the screen matches \(p("image")) by \(p("similarity")), and at least \(p("threshold")) is needed.")
    case "run.check.mismatchDensity":
      return String(
        localized: "run.check.mismatchDensity",
        defaultValue: "Block “\(block)”: the screen matches \(p("image")) by \(p("similarity")), and at least \(p("threshold")) is needed. The reference is \(p("expected")) pixels and the screen \(p("actual")): the reference was saved on a display with a different pixel density (Retina or not), where text and edges are drawn differently. Run on a display like the one the reference was saved on, or save the reference again.")
    case "run.env.notChosen":
      return String(localized: "run.env.notChosen", defaultValue: "The test reads the environment value \(p("name")), but no environment is chosen. Add one to \(environmentsFileName()).")
    case "run.env.missing":
      return String(localized: "run.env.missing", defaultValue: "The environment \(p("environment")) has no value \(p("name")). Add it to \(environmentsFileName()).")
    case "run.env.unknown":
      return String(localized: "run.env.unknown", defaultValue: "There’s no environment \(p("environment")) in \(p("file")).")
    case "run.environments.unreadable":
      return String(localized: "run.environments.unreadable", defaultValue: "\(p("file")) can’t be read: \(p("reason")).")
    case "run.project.unreadable":
      return String(localized: "run.project.unreadable", defaultValue: "\(p("file")) can’t be read: \(p("reason")).")
    case "run.data.noRow":
      return String(localized: "run.data.noRow", defaultValue: "Block “\(block)”: the table \(p("file")) has \(p("rows")) rows, so there’s no row \(p("row")).")
    case "run.data.unreadable":
      return String(localized: "run.data.unreadable", defaultValue: "Block “\(block)”: can’t read the table \(p("file")): \(IssueText.dataProblem(p("problem"), p)).")
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
    case "run.var.missing":
      return String(localized: "run.var.missing", defaultValue: "Block “\(block)”: no block before it saved a value named \(p("name")).")
    case "run.value.notNumber":
      return String(localized: "run.value.notNumber", defaultValue: "Block “\(block)”: “\(p("value"))” isn’t a number, so it can’t be compared as one.")
    case "run.process.notFound":
      return String(localized: "run.process.notFound", defaultValue: "Block “\(block)”: \(p("program")) isn’t installed on this Mac.")
    case "run.process.cannotStart":
      return String(localized: "run.process.cannotStart", defaultValue: "Block “\(block)”: \(p("program")) can’t start: \(p("reason")).")
    case "run.process.timeout":
      return String(localized: "run.process.timeout", defaultValue: "Block “\(block)”: still running after \(p("seconds")) s, so it was stopped.")
    case "run.script.failed":
      return String(localized: "run.script.failed", defaultValue: "Block “\(block)”: the script ended with exit code \(code("code")).\(reason)")
    case "run.script.missingOutput":
      return String(localized: "run.script.missingOutput", defaultValue: "Block “\(block)”: the script didn’t write \(p("name")) to $JEV_OUTPUT.")
    case "run.script.badOutput":
      return String(localized: "run.script.badOutput", defaultValue: "Block “\(block)”: the output \(p("name")) should be \(outputKind(p("type"))), but the script wrote “\(p("value"))”.")
    case "run.command.exitCode":
      return String(localized: "run.command.exitCode", defaultValue: "Block “\(block)”: the command ended with exit code \(code("actual")) instead of \(p("expected")).\(reason)")
    case "run.http.failed":
      return String(localized: "run.http.failed", defaultValue: "Block “\(block)”: \(p("url")) didn’t answer: \(p("reason")).")
    case "run.http.status":
      return String(localized: "run.http.status", defaultValue: "Block “\(block)”: \(p("url")) answered with status \(p("status")) instead of \(p("expected")).\(p("body").isEmpty ? "" : "\n" + p("body"))")
    case "run.http.noValue":
      return String(localized: "run.http.noValue", defaultValue: "Block “\(block)”: the answer has no \(p("from")).")
    case "run.file.timeout" where p("state") == "exists":
      return String(localized: "run.file.timeout.exists", defaultValue: "Block “\(block)”: \(p("path")) didn’t appear within \(p("seconds")) s.")
    case "run.file.timeout" where p("state") == "missing":
      return String(localized: "run.file.timeout.missing", defaultValue: "Block “\(block)”: \(p("path")) didn’t disappear within \(p("seconds")) s.")
    case "run.file.timeout" where p("state") == "changed":
      return String(localized: "run.file.timeout.changed", defaultValue: "Block “\(block)”: \(p("path")) didn’t change within \(p("seconds")) s.")
    case "run.file.timeout":
      return String(localized: "run.file.timeout.contains", defaultValue: "Block “\(block)”: \(p("path")) didn’t contain “\(p("text"))” within \(p("seconds")) s.")
    case "run.database.failed":
      return String(localized: "run.database.failed", defaultValue: "Block “\(block)”: the query failed: \(p("reason")).")
    case "run.database.value":
      return String(localized: "run.database.value", defaultValue: "Block “\(block)”: the query gave “\(p("actual"))” instead of “\(p("expected"))” for \(p("seconds")) s.")
    case "run.database.rows":
      return String(localized: "run.database.rows", defaultValue: "Block “\(block)”: the query gave \(p("actual")) rows instead of \(p("expected")) for \(p("seconds")) s.")
    default:
      return describeFailure(failure: failure)
    }
  }

  /// The run stopped because the AI model isn't set up right: Settings → AI can fix it.
  /// Which way an `if` step went, named as the canvas names the ways out:
  /// Took “Condition 2: Inbox is empty”, Took “Otherwise”. `nil` for other blocks.
  static func branch(_ step: Step) -> String? {
    guard let branch = step.branch else { return nil }
    let index = elseIfIndex(edge: branch).map(Int.init)
    // Reports from before an `if` had more conditions don't say how many ways out it had.
    let plain = step.branches.map { $0 <= 2 } ?? (index == nil)
    let name: String
    switch branch {
    case "then": name = plain ? String(localized: "Yes") : String(localized: "Condition \(1)")
    case "else": name = plain ? String(localized: "No") : String(localized: "Otherwise")
    default:
      guard let index else { return nil }
      name = String(localized: "Condition \(index + 2)")
    }
    guard let condition = step.branchCondition else { return String(localized: "Took “\(name)”") }
    return String(localized: "Took “\(name): \(condition)”")
  }

  static func needsAISetup(_ failure: Failure) -> Bool {
    [
      "run.compare.meaningUnavailable", "run.vision.cloudBlocked", "run.vision.noPictures", "run.vision.noModel", "run.vision.noKey",
      "run.vision.badAddress", "run.vision.badKey", "run.vision.unknownModel", "run.vision.unreachable",
      "run.vision.limitReached",
      "run.target.noLocator", "run.vision.cannotLocate", "run.vision.modelCannotLocate",
    ].contains(failure.code)
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
  /// How the tests of a batch did, in a sentence.
  static func batchOutcome(_ result: RunCenter.BatchResult) -> String {
    var counts = [String(localized: "Passed: \(result.passed)"), String(localized: "Failed: \(result.failed)")]
    if result.quarantined > 0 { counts.append(String(localized: "Failed in quarantine: \(result.quarantined)")) }
    if result.unchecked > 0 { counts.append(String(localized: "Nothing checked: \(result.unchecked)")) }
    if result.blocked > 0 { counts.append(String(localized: "Blocked: \(result.blocked)")) }
    let notRun = result.total - result.passed - result.failed - result.quarantined - result.unchecked - result.blocked
    if notRun > 0 { counts.append(String(localized: "Not run: \(notRun)")) }
    if result.added > 0 { counts.append(String(localized: "Added as dependencies: \(result.added)")) }
    let summary = counts.joined(separator: " · ")
    return result.stopped
      ? String(localized: "\(result.title) stopped. \(summary)")
      : String(localized: "\(result.title) finished. \(summary)")
  }

  /// Why a test didn't run, such as “Blocked: Sign in failed”.
  static func blocker(_ blocker: Blocker) -> String {
    switch blocker.reason {
    case .failed: String(localized: "Blocked: \(blocker.causeName) failed")
    case .notRun: String(localized: "Blocked: \(blocker.causeName) couldn’t run")
    case .missing: String(localized: "Blocked: \(blocker.cause) doesn’t exist")
    case .cycle(let tests):
      String(localized: "Blocked: the tests depend on each other in a circle: \(tests.joined(separator: " → "))")
    }
  }

  /// `sentence` as a heading: headings don't end with a period, as the other ones in reports.
  static func heading(_ sentence: String) -> String {
    sentence.hasSuffix(".") && !sentence.hasSuffix("..") ? String(sentence.dropLast()) : sentence
  }

  static func outcome(_ report: RunReport) -> String {
    switch report.verdict {
    case .cancelled:
      return String(localized: "You stopped the test.")
    case .blocked:
      guard let blocker = report.blocked else {
        return String(localized: "The test didn’t run: a test it depends on didn’t pass.")
      }
      return String(localized: "\(Self.blocker(blocker)). The test didn’t run.")
    case .failed:
      let attempt = decidingAttempt(report: report)
      let text = attempt?.failure.map(failure) ?? String(localized: "The test failed.")
      guard let row = attempt?.row else { return text }
      return String(localized: "\(RunChoices.rowTitle(number: Int(row.number), label: row.label)): \(text)")
    case .passed:
      let time = RunHistory.duration(report.durationMs)
      if report.nothingChecked {
        return String(localized: "Finished in \(time), but nothing was checked: the test has no Check block, so it can’t catch a wrong result.")
      }
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

  /// `text`, `number` or `boolean` from the core, in words.
  private static func outputKind(_ kind: String) -> String {
    switch kind {
    case "number": String(localized: "a number")
    case "boolean": String(localized: "true or false")
    default: String(localized: "text")
    }
  }
}

/// Problems with an AI model in words: `run.vision.*` codes, without the block they happened in.
@MainActor
enum VisionText {
  static func failure(_ failure: Failure) -> String {
    let p = { (name: String) in failure.params[name] ?? "" }
    let provider = p("provider")
    switch failure.code {
    case "run.vision.noPictures":
      return String(
        localized: "run.vision.noPictures",
        defaultValue:
          "The AI model for \(provider) doesn’t understand pictures, so it can’t check this screen. Turn on “Understands pictures” in Settings → AI or pick another provider."
      )
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
    case "run.vision.limitReached" where p("period") == "month":
      return String(
        localized: "run.vision.limitReached.month",
        defaultValue: "\(provider) has used $\(p("spent")) of its monthly limit of $\(p("limit")). AI checks with it stop until next month, or until the limit is raised in Settings → AI.")
    case "run.vision.limitReached":
      return String(
        localized: "run.vision.limitReached",
        defaultValue: "\(provider) has used $\(p("spent")) of its daily limit of $\(p("limit")). AI checks with it stop until tomorrow, or until the limit is raised in Settings → AI.")
    case "run.vision.limitNear" where p("period") == "month":
      return String(
        localized: "run.vision.limitNear.month",
        defaultValue: "\(provider) has used $\(p("spent")) of its monthly limit of $\(p("limit")).")
    case "run.vision.limitNear":
      return String(
        localized: "run.vision.limitNear", defaultValue: "\(provider) has used $\(p("spent")) of its daily limit of $\(p("limit")).")
    case "run.vision.journalFailed":
      return String(
        localized: "run.vision.journalFailed",
        defaultValue: "Can’t write down the AI call in \(p("path")): \(p("reason")). Without it the spending limits can’t be kept, so AI checks stop.")
    case "run.vision.badAnswer":
      return String(localized: "run.vision.badAnswer", defaultValue: "\(provider) gave an answer JEVision can’t read: \(p("answer"))")
    case "run.vision.cannotLocate":
      return String(
        localized: "run.vision.cannotLocate",
        defaultValue: "\(provider) can’t find elements by description; only JEV (TypeSafe AI) can. Set up JEV in Settings → AI.")
    case "run.vision.modelCannotLocate":
      return String(
        localized: "run.vision.modelCannotLocate",
        defaultValue:
          "The JEV model “\(p("model"))” can’t find elements by description; jev-1.14.0 and newer or jev-latest can. Choose one in Settings → AI.")
    default:
      return describeFailure(failure: failure)
    }
  }

  static func error(_ error: Error) -> String {
    switch error {
    case VisionCallError.Failed(let problem, _): failure(problem)
    default: error.localizedDescription
    }
  }
}
