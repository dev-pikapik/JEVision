import AppKit
import JEVisionCore
import JEVisionHands
import Observation
import SwiftUI

/// Runs tests from the app: checks that a run can start, gets the window out of the way, shows
/// the HUD while the test drives the Mac, and brings the window back with the result.
@MainActor
@Observable
final class RunCenter {
  struct Request: Identifiable {
    let id = UUID()
    let document: FlowDocument
    let project: URL
    /// Block to start from instead of the test's first one.
    let from: String?
    let stepByStep: Bool
    /// The paired device that asked for the run from afar.
    var requestedBy: String? = nil
  }

  struct Result: Equatable {
    let test: URL
    let project: URL
    let verdict: Verdict?
    let message: String
    /// The run's folder, for its report.
    let run: URL?
    /// The permission the run stopped for, if it did.
    let permission: Permission?
    var offersGitIgnore: Bool
    /// The run stopped for something Settings → AI fixes.
    var needsAISetup = false
  }

  /// The run that waits for a permission, a secret or a fix; shown as a sheet.
  var blocked: Request?
  /// The test that runs now, while it runs.
  private(set) var running: Request?
  /// The current or last run, for the HUD and the canvas.
  private(set) var state: RunState?
  /// The test `state` belongs to.
  private(set) var stateURL: URL?
  private(set) var result: Result?
  /// Blocks to pause before, by test file. Not part of the test: kept in this Mac's settings.
  private(set) var breakpoints: [URL: Set<String>]

  @ObservationIgnored private var run: TestRun?
  @ObservationIgnored private let input = InputWatcher()
  @ObservationIgnored private var hud: GlassPanel?
  /// Windows put away for the run.
  @ObservationIgnored private var hiddenWindows: [NSWindow] = []

  @ObservationIgnored private let defaults: UserDefaults
  /// Also hears every run of the project at the URL, for remote access.
  @ObservationIgnored var remoteListener: ((URL) -> (any RunListener)?)?

  private static let declinedKey = "gitIgnoreDeclined"
  private static let breakpointsKey = "breakpoints"

  init(defaults: UserDefaults = .standard) {
    self.defaults = defaults
    let saved = defaults.dictionary(forKey: Self.breakpointsKey) as? [String: [String]] ?? [:]
    breakpoints = Dictionary(uniqueKeysWithValues: saved.map { (URL(filePath: $0.key), Set($0.value)) })
  }

  var isRunning: Bool { running != nil }

  // MARK: Starting

  /// What keeps `request` from running now: missing permissions, secrets and environment values
  /// unset in the chosen environment, errors in the test.
  static func missing(for request: Request) -> (permissions: [Permission], values: MissingValues, errors: Int) {
    let permissions = Permission.allCases.filter { !$0.isGranted }
    let environments = ProjectEnvironments.read(request.project)
    let values = request.document.flow.map {
      MissingValues(for: $0, in: environments, isSet: { SecretStore.isSet($0, environment: $1) })
    } ?? MissingValues()
    let errors = request.document.issues.count { $0.severity == .error }
    return (permissions, values, errors)
  }

  /// Runs the test, or asks for what it needs first.
  func run(_ document: FlowDocument, in project: URL, from block: String? = nil, stepByStep: Bool = false) {
    guard !isRunning, document.flow != nil else { return }
    let request = Request(document: document, project: project, from: block, stepByStep: stepByStep)
    let missing = Self.missing(for: request)
    if !missing.permissions.isEmpty || !missing.values.isEmpty || missing.errors > 0 || !start(request) {
      blocked = request
    }
  }

  /// Runs `test`, relative to `project`, for a paired device, or says why it can't. Nothing
  /// waits for a person here: what a local run would ask for refuses the remote one.
  func runRemotely(_ test: String, in project: URL, requestedBy device: String) -> RemoteError? {
    guard !isRunning, blocked == nil else { return .RunInProgress }
    let request = Request(
      document: FlowDocument(url: project.appending(path: test)), project: project, from: nil, stepByStep: false,
      requestedBy: device)
    let missing = Self.missing(for: request)
    guard request.document.flow != nil, missing.errors == 0 else { return .TestUnreadable }
    guard missing.permissions.isEmpty, missing.values.isEmpty, start(request) else { return .HostFailed }
    return nil
  }

  /// Starts the run the sheet was holding back, now that nothing is missing.
  func confirm() {
    guard let request = blocked else { return }
    blocked = nil
    run(request.document, in: request.project, from: request.from, stepByStep: request.stepByStep)
  }

  /// Starts the run; `false` when the emergency stop can't be armed.
  private func start(_ request: Request) -> Bool {
    let run = TestRun()
    let ai = AISettings(defaults)
    let videos = VideoSettings(defaults)
    // Esc Esc stops the run right on the watcher's thread, whatever the main thread is doing.
    guard input.start(onEmergencyStop: { run.cancel() }) else { return false }
    input.ignoreInput(for: Self.handsOffTime)
    self.run = run
    running = request
    result = nil
    state = RunState()
    stateURL = request.document.url
    run.setStepMode(on: request.stepByStep)
    // Breakpoints are for debugging here; a run from afar would wait at them for nobody.
    if request.requestedBy == nil {
      run.setBreakpoints(blocks: Array(breakpoints[request.document.url] ?? []))
    }
    let hud = GlassPanel(RunHUD(center: self), width: RunHUD.width, height: RunHUD.height)
    self.hud = hud
    getOutOfTheWay()
    // The run's thread waits until the panel has stepped aside, so the click can't land on it.
    let clicking: @Sendable (CGRect?) -> Void = { [weak hud] window in
      DispatchQueue.main.sync { MainActor.assumeIsolated { hud?.clicking(into: window) } }
    }
    let choices = request.document.runChoices
    let folder = request.document.url.deletingLastPathComponent()
    let table = choices.dataRow == nil ? nil : request.document.flow.flatMap { RunChoices.dataTable(of: $0, folder: folder) }
    run.start(
      project: request.project.path, test: request.document.url.path,
      options: RunOptions(
        startAt: request.from, retries: choices.retries.map(UInt32.init),
        environment: ProjectEnvironments.read(request.project).selected?.name, dataRow: choices.row(in: table),
        vision: ai.settings(), localOnly: ai.localOnly,
        requestedBy: request.requestedBy, video: videos.mode, keepVideosDays: videos.keepDays),
      hands: MacHands(input: input, isCancelled: { run.isCancelled() }, clicking: clicking),
      secrets: StoredSecrets(), listener: RunBridge(center: self, remote: remoteListener?(request.project)))
    return true
  }

  // MARK: Control

  func pause() { run?.pause() }

  func resume() {
    input.ignoreInput(for: Self.handsOffTime)
    getOutOfTheWay()
    run?.resume()
  }

  func step() {
    input.ignoreInput(for: Self.handsOffTime)
    getOutOfTheWay()
    run?.step()
  }

  func stop() { run?.cancel() }

  /// Time to take the hand off the mouse after pressing Run, Resume or Step.
  private static let handsOffTime: TimeInterval = 1.5

  /// Brings the window back during a run, to look at the canvas.
  func showApp() {
    for window in hiddenWindows { window.orderFront(nil) }
    hiddenWindows = []
    activate()
  }

  // MARK: Breakpoints

  func hasBreakpoint(_ block: String, in document: FlowDocument) -> Bool {
    breakpoints[document.url]?.contains(block) ?? false
  }

  func toggleBreakpoint(_ block: String, in document: FlowDocument) {
    // Blocks deleted or renamed since don't stay behind.
    var set = (breakpoints[document.url] ?? []).filter { document.block($0) != nil }
    if set.remove(block) == nil { set.insert(block) }
    breakpoints[document.url] = set.isEmpty ? nil : set
    if running?.document.url == document.url { run?.setBreakpoints(blocks: Array(set)) }
    let saved = Dictionary(uniqueKeysWithValues: breakpoints.map { ($0.key.path, $0.value.sorted()) })
    defaults.set(saved, forKey: Self.breakpointsKey)
  }

  // MARK: Results

  /// How each block of `document` did in the last run, and the block running now.
  func marks(for document: FlowDocument) -> (marks: [String: RunState.Mark], current: String?) {
    guard let state, stateURL == document.url else { return ([:], nil) }
    return (state.marks, isRunning ? state.current : nil)
  }

  /// Forgets the last run's marks once the test changes.
  func clearMarks(of document: FlowDocument) {
    guard !isRunning, stateURL == document.url else { return }
    state = nil
    stateURL = nil
  }

  func dismissResult() { result = nil }

  func addToGitIgnore() throws {
    guard let project = result?.project else { return }
    try GitIgnore.add(to: project)
    result?.offersGitIgnore = false
  }

  func declineGitIgnore() {
    guard let project = result?.project else { return }
    let declined = defaults.stringArray(forKey: Self.declinedKey) ?? []
    defaults.set(declined + [project.path], forKey: Self.declinedKey)
    result?.offersGitIgnore = false
  }

  // MARK: Events

  fileprivate func apply(_ event: RunEvent) {
    guard isRunning else { return }
    let wasPaused = state?.isPaused ?? false
    state?.apply(event)
    guard let state else { return }
    if state.isPaused, !wasPaused {
      AccessibilityNotification.Announcement(RunHUD.status(of: state)).post()
    }
  }

  fileprivate func finish(_ outcome: RunOutcome) {
    guard let request = running else { return }
    input.stop()
    run = nil
    running = nil
    hud?.close()
    hud = nil
    showApp()
    let declined = defaults.stringArray(forKey: Self.declinedKey) ?? []
    let offers = !declined.contains(request.project.path) && GitIgnore.needsEntry(in: request.project)
    switch outcome {
    case .finished(let dir, _, let report):
      let failure = report.attempts.last?.failure
      result = Result(
        test: request.document.url, project: request.project, verdict: report.verdict,
        message: RunText.outcome(report), run: URL(filePath: dir, directoryHint: .isDirectory),
        permission: failure.flatMap(RunText.permission), offersGitIgnore: offers,
        needsAISetup: failure.map(RunText.needsAISetup) ?? false)
    case .failed(let reason):
      result = Result(
        test: request.document.url, project: request.project, verdict: nil,
        message: String(localized: "The test couldn’t start: \(reason)"), run: nil, permission: nil,
        offersGitIgnore: false)
    }
    if let result { AccessibilityNotification.Announcement(result.message).post() }
  }

  // MARK: Windows

  private func getOutOfTheWay() {
    for window in NSApp.windows where window.isVisible && !(window is NSPanel) {
      window.orderOut(nil)
      hiddenWindows.append(window)
    }
    NSApp.deactivate()
    hud?.show()
  }

  /// Brings JEVision to the front even though another app is active: the run left the tested app there.
  private func activate() {
    let configuration = NSWorkspace.OpenConfiguration()
    configuration.activates = true
    NSWorkspace.shared.openApplication(at: Bundle.main.bundleURL, configuration: configuration)
  }
}

/// Passes a run's progress from its thread to the main thread, in order. Remote access hears it
/// right away, on the run's thread.
private final class RunBridge: RunListener {
  let center: RunCenter
  let remote: (any RunListener)?

  init(center: RunCenter, remote: (any RunListener)?) {
    self.center = center
    self.remote = remote
  }

  func event(event: RunEvent) {
    remote?.event(event: event)
    DispatchQueue.main.async { [center] in
      MainActor.assumeIsolated { center.apply(event) }
    }
  }

  func finished(outcome: RunOutcome) {
    remote?.finished(outcome: outcome)
    DispatchQueue.main.async { [center] in
      MainActor.assumeIsolated { center.finish(outcome) }
    }
  }
}
