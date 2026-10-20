import Foundation
import JEVisionCore
import JEVisionHands

/// `jevision mcp`: lets AI assistants on this Mac read, edit and run the project's tests. The
/// server itself lives in the core; this side gives it the Mac: settings, hands and secrets.
struct MCPCommand {
  let project: String?

  func execute() -> ExitStatus {
    guard let folder = project ?? Self.defaultProject() else {
      Console.standardError.line(
        "jevision: name the project folder with --project: the assistant started jevision in /, which is no project.")
      return .failure
    }
    do {
      try serveAssistants(project: folder, host: AssistantBridge())
      return .ok
    } catch AssistantServerError.NoProject(let path) {
      Console.standardError.line("jevision: there is no folder “\(path)”")
    } catch {
      Console.standardError.line("jevision: \(error.localizedDescription)")
    }
    return .failure
  }

  /// The nearest folder at or above the current one that has a `.jevision` folder, as the app
  /// makes one in the project it opens, or the environments file; otherwise the current folder, unless it is `/`.
  static func defaultProject(from current: String = FileManager.default.currentDirectoryPath) -> String? {
    var folder = current
    while folder != "/" {
      if isProjectFolder(folder) { return folder }
      folder = (folder as NSString).deletingLastPathComponent
    }
    return current == "/" ? nil : current
  }
}

/// The Mac's side of the server. Reads the settings at every call, so a switch turned off in
/// the app applies at once.
final class AssistantBridge: AssistantHost, @unchecked Sendable {
  private let lock = NSLock()
  /// Watches the person's mouse and keyboard while a run goes.
  private var input: InputWatcher?

  func permissions() -> AssistantPermissions {
    let settings = AISettings.app
    settings.refresh()
    return AssistantPermissions(
      runTests: settings.assistantsMayRun, screenshots: settings.assistantsMayCapture,
      askFirst: settings.assistantsAskFirst)
  }

  func sharedFolder() -> String {
    AISettings.assistantsFolder.path
  }

  func readinessProblem() -> String? {
    Readiness.problem()
  }

  func startRun(run: TestRun) throws -> Hands {
    let input = InputWatcher()
    // Esc Esc stops the run right on the watcher's thread.
    guard input.start(onEmergencyStop: { run.cancel() }) else {
      throw HandsError.Failed(
        reason: "macOS didn’t let jevision watch the keyboard, so pressing Esc twice couldn’t stop the test. The person at this Mac can allow Accessibility for the app that started jevision.")
    }
    lock.withLock { self.input = input }
    return MacHands(input: input, isCancelled: { run.isCancelled() })
  }

  func runPaused(run: TestRun) {
    guard let input = lock.withLock({ input }) else { return run.resume() }
    TestSession.resumeWhenIdle(run, input: input)
  }

  func runEnded() {
    lock.withLock {
      input?.stop()
      input = nil
    }
  }

  func secrets() -> SecretSource {
    StoredSecrets()
  }

  func runOptions() -> RunOptions {
    let ai = AISettings.app
    let videos = VideoSettings.app
    return RunOptions(
      startAt: nil, retries: nil, vision: ai.settings(), localOnly: ai.localOnly, fallback: ai.fallback(),
      providers: ai.namedProviders, usage: ai.usage, video: videos.mode, keepVideosDays: videos.keepDays)
  }

  func capture(app: String) throws -> Screenshot {
    if MacHands.screenIsLocked { throw HandsError.ScreenLocked }
    return try MacHands(input: InputWatcher(), isCancelled: { false }).capture(app: app)
  }
}
