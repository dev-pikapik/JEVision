import AppKit
import JEVisionCore

extension InputWatcher {
  /// How long the mouse and keyboard must rest before a paused test goes on.
  public static let idleTime: TimeInterval = 3

  /// A person took the mouse or keyboard and the test paused: it goes on once they let go for
  /// `idleTime`, unless the run is stopped first.
  public func resumeWhenIdle(_ run: TestRun) {
    Thread.detachNewThread { [self] in
      var quietSince = Date()
      while !run.isCancelled() {
        Thread.sleep(forTimeInterval: 0.25)
        if takeUserInput() {
          quietSince = Date()
        } else if Date().timeIntervalSince(quietSince) >= Self.idleTime {
          run.resume()
          return
        }
      }
    }
  }
}

/// The Mac's side of the assistant server, for the assistants that run on this Mac and for the
/// ones on other computers: settings, hands and secrets. It reads the settings at every call, so
/// a switch turned off in the app applies at once.
public final class MacAssistantHost: AssistantHost, @unchecked Sendable {
  private let lock = NSLock()
  /// Watches the person's mouse and keyboard while a run goes.
  private var input: InputWatcher?
  private let defaults: UserDefaults
  private let readiness: @Sendable () -> String?

  /// `readiness` says what stands in the way of a run now, in words for the assistant to pass on,
  /// or `nil`; it is asked before every run, and again as the run starts.
  public init(defaults: UserDefaults = Profile.current.appDefaults, readiness: @escaping @Sendable () -> String?) {
    self.defaults = defaults
    self.readiness = readiness
  }

  public func permissions() -> AssistantPermissions {
    guard (try? AppPreferences.check(defaults)) != nil else {
      return AssistantPermissions(runTests: false, screenshots: false, askFirst: true)
    }
    let settings = AISettings(defaults)
    settings.refresh()
    return AssistantPermissions(
      runTests: settings.assistantsMayRun, screenshots: settings.assistantsMayCapture,
      askFirst: settings.assistantsAskFirst)
  }

  public func sharedFolder() -> String {
    AISettings.assistantsFolder.path
  }

  public func readinessProblem() -> String? {
    do { try AppPreferences.check(defaults) } catch { return error.localizedDescription }
    return readiness()
  }

  public func startRun(run: TestRun) throws -> Hands {
    try AppPreferences.check(defaults)
    if let problem = readiness() { throw HandsError.Failed(reason: problem) }
    let input = InputWatcher()
    // Esc Esc stops the run right on the watcher's thread.
    guard input.start(onEmergencyStop: { run.cancel() }) else {
      throw HandsError.Failed(reason: handsWords("hands.keyboard.watchRefused"))
    }
    lock.withLock { self.input = input }
    return MacHands(input: input, isCancelled: { run.isCancelled() })
  }

  public func runPaused(run: TestRun) {
    guard let input = lock.withLock({ input }) else { return run.resume() }
    input.resumeWhenIdle(run)
  }

  public func runEnded() {
    lock.withLock {
      input?.stop()
      input = nil
    }
  }

  public func secrets(project: String) -> SecretSource {
    StoredSecrets(project: project)
  }

  public func runOptions() -> RunOptions {
    guard (try? AppPreferences.check(defaults)) != nil else {
      return RunOptions(startAt: nil, retries: nil, vision: nil, localOnly: true, fallback: nil,
        providers: nil, finder: nil, usage: nil, video: .off, keepVideosDays: nil, language: "en")
    }
    let ai = AISettings(defaults)
    let videos = VideoSettings(defaults)
    return RunOptions(
      startAt: nil, retries: nil, vision: ai.settings(), localOnly: ai.localOnly, fallback: ai.fallback(),
      providers: ai.namedProviders, finder: ai.finderID(), usage: ai.usage, video: videos.mode,
      keepVideosDays: videos.keepDays,
      language: RunLanguage.language(defaults))
  }

  public func capture(app: String) throws -> Screenshot {
    try AppPreferences.check(defaults)
    if MacHands.screenIsLocked { throw HandsError.ScreenLocked }
    return try MacHands(input: InputWatcher(), isCancelled: { false }).capture(app: app)
  }

  public func openApp() throws {
    guard let app = Self.app(containing: Bundle.main.executableURL) else {
      throw HandsError.AppNotFound(app: "JEVision")
    }
    let config = NSWorkspace.OpenConfiguration()
    config.activates = false
    let error = MacHands.wait { done in
      NSWorkspace.shared.openApplication(at: app, configuration: config) { _, error in
        done(error?.localizedDescription)
      }
    }
    if let error { throw HandsError.Failed(reason: error) }
  }

  /// JEVision: the app the command-line tool `tool` lives in (it is linked from elsewhere, so
  /// links are followed), or else the copy macOS knows by its bundle id.
  static func app(containing tool: URL?) -> URL? {
    let bundle = tool?.resolvingSymlinksInPath()
      .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    if let bundle, Bundle(url: bundle)?.bundleIdentifier == AISettings.appDomain { return bundle }
    return NSWorkspace.shared.urlForApplication(withBundleIdentifier: AISettings.appDomain)
  }
}
