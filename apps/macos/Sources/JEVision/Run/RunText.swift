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
      return String(localized: "run.test.unreadable", defaultValue: "Can’t read the test file \(p("test")): \(p("reason")).", bundle: .app, locale: .app)
    case "run.test.invalid":
      return String(localized: "run.test.invalid", defaultValue: "The test has problems to fix first (\(p("errors"))). They’re marked on the blocks.", bundle: .app, locale: .app)
    case "run.start.unknownBlock":
      return String(localized: "run.start.unknownBlock", defaultValue: "There’s no block “\(block)” to start from.", bundle: .app, locale: .app)
    case "run.secret.missing":
      return String(localized: "run.secret.missing", defaultValue: "The secret \(p("name")) isn’t set. Set it in Test details.", bundle: .app, locale: .app)
    case "run.input.missing":
      return String(localized: "run.input.missing", defaultValue: "Block “\(block)”: nothing gives the input \(p("name")). Run this block from a test that fills it in.", bundle: .app, locale: .app)
    case "run.block.notInstalled":
      let package = BlockNeed(failure)?.label ?? p("package")
      return String(localized: "run.block.notInstalled", defaultValue: "Block “\(block)”: the community block \(package) isn’t installed. Install it from File → Install Block…, then run the test again.", bundle: .app, locale: .app)
    case "run.block.needsNewer":
      return String(localized: "run.block.needsNewer", defaultValue: "Block “\(block)”: the community block \(p("package")) needs JEVision \(p("needs")) or newer, and this is \(p("have")). Update JEVision to run it.", bundle: .app, locale: .app)
    case "run.path.outsideProject":
      return String(localized: "run.path.outsideProject", defaultValue: "Block “\(block)”: \(p("path")) is outside the project folder.", bundle: .app, locale: .app)
    case "run.file.missing":
      return String(localized: "run.file.missing", defaultValue: "Block “\(block)”: the file \(p("file")) doesn’t exist.", bundle: .app, locale: .app)
    case "run.file.copyFailed":
      return String(localized: "run.file.copyFailed", defaultValue: "Block “\(block)”: can’t copy \(p("file")): \(p("reason")).", bundle: .app, locale: .app)
    case "run.target.notFound":
      return String(localized: "run.target.notFound", defaultValue: "Block “\(block)”: \(p("image")) wasn’t found on the screen.", bundle: .app, locale: .app)
    case "run.target.descriptionNotFound":
      return String(
        localized: "run.target.descriptionNotFound", defaultValue: "Block “\(block)”: “\(p("description"))” wasn’t found on the screen.", bundle: .app, locale: .app)
    case "run.target.noLocator":
      return String(
        localized: "run.target.noLocator",
        defaultValue: "Block “\(block)”: finding an element by its description needs JEV (TypeSafe AI). Set it up in Settings → AI.", bundle: .app, locale: .app)
    case "run.reference.missing":
      return String(localized: "run.reference.missing", defaultValue: "Block “\(block)”: the reference image \(p("image")) doesn’t exist.", bundle: .app, locale: .app)
    case "run.reference.unreadable":
      return String(localized: "run.reference.unreadable", defaultValue: "Block “\(block)”: can’t read the image \(p("image")): \(p("reason")).", bundle: .app, locale: .app)
    case "run.screenshot.unreadable":
      return String(localized: "run.screenshot.unreadable", defaultValue: "Block “\(block)”: can’t read the screenshot: \(p("reason")).", bundle: .app, locale: .app)
    case "run.compare.shapeMismatch":
      return String(
        localized: "run.compare.shapeMismatch",
        defaultValue: "Block “\(block)”: the window is \(p("actual")), but the reference \(p("image")) is \(p("expected")).", bundle: .app, locale: .app)
    case "run.compare.meaningUnavailable":
      return String(
        localized: "run.compare.meaningUnavailable",
        defaultValue: "Block “\(block)”: comparing by meaning needs an AI model. Choose one in Settings → AI.", bundle: .app, locale: .app)
    case "run.check.meaningMismatch":
      return String(
        localized: "run.check.meaningMismatch",
        defaultValue: "Block “\(block)”: the screen doesn’t show what’s expected. \(p("provider")): \(p("reason"))", bundle: .app, locale: .app)
    case "run.vision.cloudBlocked" where p("setting") == "project":
      return String(
        localized: "run.vision.cloudBlocked.project",
        defaultValue: "Block “\(block)”: this project allows only models on this Mac, and \(p("provider")) works in the cloud. Choose a local model, or turn off “Only models on this Mac for this project” in Settings → AI.", bundle: .app, locale: .app)
    case "run.vision.cloudBlocked":
      return String(
        localized: "run.vision.cloudBlocked",
        defaultValue: "Block “\(block)”: only models on this Mac are allowed, and \(p("provider")) works in the cloud. Choose a local model or turn off “Only models on this Mac” in Settings → AI.", bundle: .app, locale: .app)
    case let code where code.hasPrefix("run.vision."):
      return String(localized: "Block “\(block)”: \(VisionText.failure(failure))")
    case "run.click.noChange":
      return String(localized: "run.click.noChange", defaultValue: "Block “\(block)”: the screen didn’t change within \(p("seconds")) s after the click.", bundle: .app, locale: .app)
    case "run.action.noChange":
      return String(localized: "run.action.noChange", defaultValue: "Block “\(block)”: the screen didn’t change within \(p("seconds")) s after this step.", bundle: .app, locale: .app)
    case "run.waitFor.timeout" where p("target") == "screenStable":
      return String(localized: "run.waitFor.timeout.screenStable", defaultValue: "Block “\(block)”: the screen didn’t settle within \(p("seconds")) s.", bundle: .app, locale: .app)
    case "run.waitFor.timeout" where !p("dark").isEmpty:
      return String(
        localized: "run.waitFor.timeout.dark",
        defaultValue: "Block “\(block)”: waited \(p("seconds")) s for \(p("target")), but it didn’t happen. The window is dark and only the light picture exists; save the dark one as \(p("dark")).", bundle: .app, locale: .app)
    case "run.waitFor.timeout":
      return String(localized: "run.waitFor.timeout", defaultValue: "Block “\(block)”: waited \(p("seconds")) s for \(p("target")), but it didn’t happen.", bundle: .app, locale: .app)
    case "run.check.mismatch":
      return String(
        localized: "run.check.mismatch",
        defaultValue: "Block “\(block)”: the screen matches \(p("image")) by \(p("similarity")), and at least \(p("threshold")) is needed.", bundle: .app, locale: .app)
    case "run.check.mismatchDensity":
      return String(
        localized: "run.check.mismatchDensity",
        defaultValue: "Block “\(block)”: the screen matches \(p("image")) by \(p("similarity")), and at least \(p("threshold")) is needed. The reference is \(p("expected")) pixels and the screen \(p("actual")): the reference was saved on a display with a different pixel density (Retina or not), where text and edges are drawn differently. Run on a display like the one the reference was saved on, or save the reference again.", bundle: .app, locale: .app)
    case "run.env.notChosen":
      return String(localized: "run.env.notChosen", defaultValue: "The test reads the environment value \(p("name")), but no environment is chosen. Add one to \(environmentsFileName()).", bundle: .app, locale: .app)
    case "run.env.missing":
      return String(localized: "run.env.missing", defaultValue: "The environment \(p("environment")) has no value \(p("name")). Add it to \(environmentsFileName()).", bundle: .app, locale: .app)
    case "run.env.unknown":
      return String(localized: "run.env.unknown", defaultValue: "There’s no environment \(p("environment")) in \(p("file")).", bundle: .app, locale: .app)
    case "run.environments.unreadable":
      return String(localized: "run.environments.unreadable", defaultValue: "\(p("file")) can’t be read: \(p("reason")).", bundle: .app, locale: .app)
    case "run.project.unreadable":
      return String(localized: "run.project.unreadable", defaultValue: "\(p("file")) can’t be read: \(p("reason")).", bundle: .app, locale: .app)
    case "run.data.noRow":
      return String(localized: "run.data.noRow", defaultValue: "Block “\(block)”: the table \(p("file")) has \(p("rows")) rows, so there’s no row \(p("row")).", bundle: .app, locale: .app)
    case "run.data.unreadable":
      return String(localized: "run.data.unreadable", defaultValue: "Block “\(block)”: can’t read the table \(p("file")): \(IssueText.dataProblem(p("problem"), p)).", bundle: .app, locale: .app)
    case "run.data.unknownColumn":
      return String(localized: "run.data.unknownColumn", defaultValue: "Block “\(block)”: there’s no table column “\(p("column"))” here.", bundle: .app, locale: .app)
    case "run.runTest.recursion":
      return String(
        localized: "run.runTest.recursion",
        defaultValue: "Block “\(block)”: the test \(p("test")) is already running, and a test can’t run itself.", bundle: .app, locale: .app)
    case "run.io.writeFailed":
      return String(localized: "run.io.writeFailed", defaultValue: "Can’t save \(p("path")): \(p("reason")).", bundle: .app, locale: .app)
    case "run.hands.permissionDenied":
      let name = permission(of: failure)?.title ?? p("permission")
      return String(localized: "run.hands.permissionDenied", defaultValue: "JEVision isn’t allowed to use \(name). Turn it on in Settings → Permissions.", bundle: .app, locale: .app)
    case "run.hands.appNotFound":
      return String(localized: "run.hands.appNotFound", defaultValue: "Block “\(block)”: \(app) isn’t installed on this Mac.", bundle: .app, locale: .app)
    case "run.hands.noWindow":
      return String(localized: "run.hands.noWindow", defaultValue: "Block “\(block)”: \(app) has no open window.", bundle: .app, locale: .app)
    case "run.hands.cannotOpen":
      return String(localized: "run.hands.cannotOpen", defaultValue: "Block “\(block)”: can’t open \(p("file")).", bundle: .app, locale: .app)
    case "run.hands.screenLocked":
      return String(localized: "run.hands.screenLocked", defaultValue: "Block “\(block)”: the screen is locked. Unlock the Mac and run the test again.", bundle: .app, locale: .app)
    case "run.hands.stillRunning":
      return String(
        localized: "run.hands.stillRunning",
        defaultValue: "Block “\(block)”: \(app) didn’t quit, so it can’t start over. Close it, or answer what it asks, and run the test again.", bundle: .app, locale: .app)
    case "run.hands.windowFixed":
      return String(
        localized: "run.hands.windowFixed",
        defaultValue: "Block “\(block)”: the window of \(app) doesn’t allow its size or position to be changed. Turn off the fixed size and position in this block.", bundle: .app, locale: .app)
    case "run.hands.windowDiffers" where p("what") == "position":
      return String(
        localized: "run.hands.windowDiffers.position",
        defaultValue: "Block “\(block)”: the window of \(app) should be at \(p("wanted")) but is at \(p("actual")). The screen may be too small, or the app keeps its window where it likes.", bundle: .app, locale: .app)
    case "run.hands.windowDiffers":
      return String(
        localized: "run.hands.windowDiffers.size",
        defaultValue: "Block “\(block)”: the window of \(app) should be \(p("wanted")) points but is \(p("actual")). The app may have its own limits, or the screen may be too small.", bundle: .app, locale: .app)
    case "run.hands.failed":
      return String(localized: "run.hands.failed", defaultValue: "Block “\(block)”: \(p("reason"))", bundle: .app, locale: .app)
    case "run.var.missing":
      return String(localized: "run.var.missing", defaultValue: "Block “\(block)”: no block before it saved a value named \(p("name")).", bundle: .app, locale: .app)
    case "run.value.notNumber":
      return String(localized: "run.value.notNumber", defaultValue: "Block “\(block)”: “\(p("value"))” isn’t a number, so it can’t be compared as one.", bundle: .app, locale: .app)
    case "run.process.notFound":
      return String(localized: "run.process.notFound", defaultValue: "Block “\(block)”: \(p("program")) isn’t installed on this Mac.", bundle: .app, locale: .app)
    case "run.process.cannotStart":
      return String(localized: "run.process.cannotStart", defaultValue: "Block “\(block)”: \(p("program")) can’t start: \(p("reason")).", bundle: .app, locale: .app)
    case "run.process.timeout":
      return String(localized: "run.process.timeout", defaultValue: "Block “\(block)”: still running after \(p("seconds")) s, so it was stopped.", bundle: .app, locale: .app)
    case "run.script.failed":
      return String(localized: "run.script.failed", defaultValue: "Block “\(block)”: the script ended with exit code \(code("code")).\(reason)", bundle: .app, locale: .app)
    case "run.script.missingOutput":
      return String(localized: "run.script.missingOutput", defaultValue: "Block “\(block)”: the script didn’t write \(p("name")) to $JEV_OUTPUT.", bundle: .app, locale: .app)
    case "run.script.badOutput":
      return String(localized: "run.script.badOutput", defaultValue: "Block “\(block)”: the output \(p("name")) should be \(outputKind(p("type"))), but the script wrote “\(p("value"))”.", bundle: .app, locale: .app)
    case "run.script.badInput":
      return String(localized: "run.script.badInput", defaultValue: "Block “\(block)”: the input \(p("name")) should be \(outputKind(p("type"))), but it is “\(p("value"))”.", bundle: .app, locale: .app)
    case "run.clipboard.mismatch":
      return String(localized: "run.clipboard.mismatch", defaultValue: "Block “\(block)”: waited \(p("seconds")) s for this to be true: \(clipboardExpectation(p("test"), p("expected"))). The clipboard holds “\(p("actual"))”.", bundle: .app, locale: .app)
    case "run.clipboard.notPicture":
      return String(localized: "run.clipboard.notPicture", defaultValue: "Block “\(block)”: \(p("image")) isn’t a PNG picture, so it can’t go on the clipboard.", bundle: .app, locale: .app)
    case "run.clipboard.noImage":
      return String(localized: "run.clipboard.noImage", defaultValue: "Block “\(block)”: the clipboard holds no picture.", bundle: .app, locale: .app)
    case "run.clipboard.noFiles":
      return String(localized: "run.clipboard.noFiles", defaultValue: "Block “\(block)”: the clipboard holds no copied files.", bundle: .app, locale: .app)
    case "run.target.badArea":
      return String(localized: "run.target.badArea", defaultValue: "Block “\(block)”: “\(p("value"))” isn’t an area. It must be four numbers from 0 to 1 as x,y,width,height, such as 0.1,0.2,0.5,0.3.", bundle: .app, locale: .app)
    case "run.dateTime.failed":
      return String(localized: "run.dateTime.failed", defaultValue: "Block “\(block)”: \(p("reason")).", bundle: .app, locale: .app)
    case "run.command.exitCode":
      return String(localized: "run.command.exitCode", defaultValue: "Block “\(block)”: the command ended with exit code \(code("actual")) instead of \(p("expected")).\(reason)", bundle: .app, locale: .app)
    case "run.http.failed":
      return String(localized: "run.http.failed", defaultValue: "Block “\(block)”: \(p("url")) didn’t answer: \(p("reason")).", bundle: .app, locale: .app)
    case "run.http.status":
      return String(localized: "run.http.status", defaultValue: "Block “\(block)”: \(p("url")) answered with status \(p("status")) instead of \(p("expected")).\(p("body").isEmpty ? "" : "\n" + p("body"))", bundle: .app, locale: .app)
    case "run.http.noValue":
      return String(localized: "run.http.noValue", defaultValue: "Block “\(block)”: the answer has no \(p("from")).", bundle: .app, locale: .app)
    case "run.file.timeout" where p("state") == "exists":
      return String(localized: "run.file.timeout.exists", defaultValue: "Block “\(block)”: \(p("path")) didn’t appear within \(p("seconds")) s.", bundle: .app, locale: .app)
    case "run.file.timeout" where p("state") == "missing":
      return String(localized: "run.file.timeout.missing", defaultValue: "Block “\(block)”: \(p("path")) didn’t disappear within \(p("seconds")) s.", bundle: .app, locale: .app)
    case "run.file.timeout" where p("state") == "changed":
      return String(localized: "run.file.timeout.changed", defaultValue: "Block “\(block)”: \(p("path")) didn’t change within \(p("seconds")) s.", bundle: .app, locale: .app)
    case "run.file.timeout":
      return String(localized: "run.file.timeout.contains", defaultValue: "Block “\(block)”: \(p("path")) didn’t contain “\(p("text"))” within \(p("seconds")) s.", bundle: .app, locale: .app)
    case "run.database.failed":
      return String(localized: "run.database.failed", defaultValue: "Block “\(block)”: the query failed: \(p("reason")).", bundle: .app, locale: .app)
    case "run.database.value":
      return String(localized: "run.database.value", defaultValue: "Block “\(block)”: the query gave “\(p("actual"))” instead of “\(p("expected"))” for \(p("seconds")) s.", bundle: .app, locale: .app)
    case "run.database.rows":
      return String(localized: "run.database.rows", defaultValue: "Block “\(block)”: the query gave \(p("actual")) rows instead of \(p("expected")) for \(p("seconds")) s.", bundle: .app, locale: .app)
    default:
      return describeFailure(failure: failure)
    }
  }

  /// The run stopped because the AI model isn't set up right: Settings → AI can fix it.
  /// Who has the screen, for a run that waits for its turn: Screen busy: JEVision, test Sign in, since 12:03.
  static func screenBusy(_ holder: ScreenHolder?) -> String {
    guard let holder else {
      return String(localized: "screen.busy.unknown", defaultValue: "Screen busy: another run is using it.", bundle: .app, locale: .app)
    }
    let since = Date(timeIntervalSince1970: TimeInterval(holder.since)).formatted(date: .omitted, time: .shortened)
    return String(
      localized: "screen.busy", defaultValue: "Screen busy: \(holder.process), test \(holder.test), since \(since)", bundle: .app, locale: .app)
  }

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
    case .unknown:
      return String(localized: "A newer version of JEVision saved this run. Update the app to see how it ended.")
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
    case "image": String(localized: "the path of a PNG picture")
    case "area": String(localized: "an area written as x,y,width,height in fractions of the window")
    default: String(localized: "text")
    }
  }

  /// What a clipboard check expected, as a sentence part: `test` is the check's name in the core.
  private static func clipboardExpectation(_ test: String, _ expected: String) -> String {
    switch test {
    case "notEquals": String(localized: "the clipboard is different from “\(expected)”")
    case "contains": String(localized: "the clipboard contains “\(expected)”")
    case "greaterThan": String(localized: "the clipboard is a number greater than \(expected)")
    case "lessThan": String(localized: "the clipboard is a number less than \(expected)")
    default: String(localized: "the clipboard is equal to “\(expected)”")
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
          "The AI model for \(provider) doesn’t understand pictures, so it can’t check this screen. Turn on “Understands pictures” in Settings → AI or pick another provider.", bundle: .app, locale: .app
      )
    case "run.vision.noModel":
      return String(localized: "run.vision.noModel", defaultValue: "No model is chosen for \(provider). Choose one in Settings → AI.", bundle: .app, locale: .app)
    case "run.vision.noKey":
      return String(localized: "run.vision.noKey", defaultValue: "The API key for \(provider) isn’t set. Set it in Settings → AI.", bundle: .app, locale: .app)
    case "run.vision.badAddress":
      return String(
        localized: "run.vision.badAddress", defaultValue: "“\(p("address"))” isn’t a web address. It should start with http:// or https://.", bundle: .app, locale: .app)
    case "run.vision.unreachable":
      return String(
        localized: "run.vision.unreachable",
        defaultValue: "Can’t reach \(provider) at \(p("address")). Check that it’s running and the Mac is online.", bundle: .app, locale: .app)
    case "run.vision.timeout":
      return String(localized: "run.vision.timeout", defaultValue: "\(provider) didn’t answer within \(p("seconds")) s.", bundle: .app, locale: .app)
    case "run.vision.badKey":
      return String(localized: "run.vision.badKey", defaultValue: "\(provider) didn’t accept the API key. Set it again in Settings → AI.", bundle: .app, locale: .app)
    case "run.vision.unknownModel":
      return String(localized: "run.vision.unknownModel", defaultValue: "\(provider) has no model “\(p("model"))”. Choose another one in Settings → AI.", bundle: .app, locale: .app)
    case "run.vision.rateLimited":
      return String(
        localized: "run.vision.rateLimited",
        defaultValue: "\(provider) has had too many requests, or the account’s limit is reached. Try again later. \(p("message"))", bundle: .app, locale: .app)
    case "run.vision.serviceError":
      return String(localized: "run.vision.serviceError", defaultValue: "\(provider) answered with error \(p("status")): \(p("message"))", bundle: .app, locale: .app)
    case "run.vision.refused":
      return String(localized: "run.vision.refused", defaultValue: "\(provider) declined to judge the screen: \(p("message"))", bundle: .app, locale: .app)
    case "run.vision.limitReached" where p("period") == "month":
      return String(
        localized: "run.vision.limitReached.month",
        defaultValue: "\(provider) has used $\(p("spent")) of its monthly limit of $\(p("limit")). AI checks with it stop until next month, or until the limit is raised in Settings → AI.", bundle: .app, locale: .app)
    case "run.vision.limitReached":
      return String(
        localized: "run.vision.limitReached",
        defaultValue: "\(provider) has used $\(p("spent")) of its daily limit of $\(p("limit")). AI checks with it stop until tomorrow, or until the limit is raised in Settings → AI.", bundle: .app, locale: .app)
    case "run.vision.limitNear" where p("period") == "month":
      return String(
        localized: "run.vision.limitNear.month",
        defaultValue: "\(provider) has used $\(p("spent")) of its monthly limit of $\(p("limit")).", bundle: .app, locale: .app)
    case "run.vision.limitNear":
      return String(
        localized: "run.vision.limitNear", defaultValue: "\(provider) has used $\(p("spent")) of its daily limit of $\(p("limit")).", bundle: .app, locale: .app)
    case "run.vision.journalFailed":
      return String(
        localized: "run.vision.journalFailed",
        defaultValue: "Can’t write down the AI call in \(p("path")): \(p("reason")). Without it the spending limits can’t be kept, so AI checks stop.", bundle: .app, locale: .app)
    case "run.vision.badAnswer":
      return String(localized: "run.vision.badAnswer", defaultValue: "\(provider) gave an answer JEVision can’t read: \(p("answer"))", bundle: .app, locale: .app)
    case "run.vision.cannotLocate":
      return String(
        localized: "run.vision.cannotLocate",
        defaultValue: "\(provider) can’t find elements by description; only JEV (TypeSafe AI) can. Set up JEV in Settings → AI.", bundle: .app, locale: .app)
    case "run.vision.modelCannotLocate":
      return String(
        localized: "run.vision.modelCannotLocate",
        defaultValue:
          "The JEV model “\(p("model"))” can’t find elements by description; jev-1.14.0 and newer or jev-latest can. Choose one in Settings → AI.", bundle: .app, locale: .app)
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
