import Foundation
import JEVisionCore

/// Run results in words. The catalog keys are the core's failure codes.
@MainActor
enum RunText {
  /// The core words every failure, in the language the app hands it. Only the names the Mac shows
  /// for applications are known here.
  static func failure(_ failure: Failure) -> String {
    var failure = failure
    if let app = failure.params["app"] { failure.params["app"] = AppCatalog.shared.name(of: app) }
    return describeFailure(failure: failure)
  }

  /// [`failure(_:)`] for a message shown beside its block. The core names the block the way the
  /// canvas does from the type the failure keeps; this gives it the type for failures saved before
  /// they kept it.
  static func failure(_ failure: Failure, blockType: String) -> String {
    var failure = failure
    if failure.params["block"] != nil, failure.params["blockType"] == nil { failure.params["blockType"] = blockType }
    return RunText.failure(failure)
  }

  /// Who has the screen, for a run that waits for its turn: Screen busy: JEVision, test Sign in, since 12:03.
  static func screenBusy(_ holder: ScreenHolder?) -> String {
    guard let holder else {
      return String(localized: "screen.busy.unknown", defaultValue: "Screen busy: another run is using it.", bundle: .app, locale: .app)
    }
    let since = Date(timeIntervalSince1970: TimeInterval(holder.since)).formatted(date: .omitted, time: .shortened)
    // The command-line tool is `jevision`: to someone who never opens a terminal that is only a word.
    if holder.process == "jevision" {
      return String(
        localized: "screen.busy.terminal", defaultValue: "Screen busy: JEVision in a terminal, test \(holder.test), since \(since)", bundle: .app,
        locale: .app)
    }
    return String(
      localized: "screen.busy", defaultValue: "Screen busy: \(holder.process), test \(holder.test), since \(since)", bundle: .app, locale: .app)
  }

  /// A block where a test fails, named as the canvas names it: its type and what it does, and the
  /// test it's in when that isn't `shownTest`, the test on screen. The test open in the editor is
  /// read as it is there, unsaved changes included.
  static func place(block: String, blockType: String, test: String, shownTest: String?, in project: ProjectStore) -> String {
    let flow: Flow? =
      if test == shownTest, project.selection == test {
        project.document?.flow
      } else {
        project.rootURL.flatMap { root in
          (try? String(contentsOf: root.appending(path: test), encoding: .utf8)).flatMap { try? loadFlow(json: $0) }
        }
      }
    var text = BlockInfo.title(blockType)
    if let kind = flow?.blocks.first(where: { $0.id == block })?.kind {
      text += " · " + BlockSummary.text(kind, appName: AppCatalog.shared.name(of:))
    }
    if test != shownTest {
      text += " · " + String(localized: "in \(ProjectScanner.testName(path: test))")
    }
    return text
  }

  /// Which way an `if` step went, named as the canvas names the ways out:
  /// Took “Condition 2: Inbox is empty”, Took “Otherwise”. `nil` for other blocks.
  static func branch(_ step: Step) -> String? { branchText(step: step) }

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
  static func blocker(_ blocker: Blocker) -> String { describeBlocker(blocker: blocker) }

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
}

/// Problems with an AI model in words: `run.vision.*` codes, without the block they happened in.
@MainActor
enum VisionText {
  static func failure(_ failure: Failure) -> String { describeVisionFailure(failure: failure) }

  static func error(_ error: Error) -> String {
    switch error {
    case VisionCallError.Failed(let problem, _): failure(problem)
    default: error.localizedDescription
    }
  }
}
