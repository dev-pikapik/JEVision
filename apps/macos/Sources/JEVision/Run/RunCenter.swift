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
  }

  struct Result: Equatable {
    let test: URL
    let project: URL
    let verdict: Verdict?
    let message: String
    let report: URL?
    /// The permission the run stopped for, if it did.
    let permission: Permission?
    var offersGitIgnore: Bool
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
  /// Blocks to pause before, by test file. Kept while the app is open.
  private(set) var breakpoints: [URL: Set<String>] = [:]

  @ObservationIgnored private var run: TestRun?
  @ObservationIgnored private let input = InputWatcher()
  @ObservationIgnored private var hud: RunPanel?
  /// Windows put away for the run.
  @ObservationIgnored private var hiddenWindows: [NSWindow] = []

  private static let declinedKey = "gitIgnoreDeclined"

  var isRunning: Bool { running != nil }

  // MARK: Starting

  /// What keeps `request` from running now: missing permissions, unset secrets, errors in the test.
  static func missing(for request: Request) -> (permissions: [Permission], secrets: [String], errors: Int) {
    let permissions = Permission.allCases.filter { !$0.isGranted }
    let secrets = request.document.flow.map(requiredSecrets).map { $0.filter { !SecretStore.isSet($0) } } ?? []
    let errors = request.document.issues.count { $0.severity == .error }
    return (permissions, secrets, errors)
  }

  /// Runs the test, or asks for what it needs first.
  func run(_ document: FlowDocument, in project: URL, from block: String? = nil, stepByStep: Bool = false) {
    guard !isRunning, document.flow != nil else { return }
    let request = Request(document: document, project: project, from: block, stepByStep: stepByStep)
    let missing = Self.missing(for: request)
    if missing.permissions.isEmpty, missing.secrets.isEmpty, missing.errors == 0 {
      start(request)
    } else {
      blocked = request
    }
  }

  /// Starts the run the sheet was holding back, now that nothing is missing.
  func confirm() {
    guard let request = blocked else { return }
    blocked = nil
    run(request.document, in: request.project, from: request.from, stepByStep: request.stepByStep)
  }

  private func start(_ request: Request) {
    let run = TestRun()
    // Esc Esc stops the run right on the watcher's thread, whatever the main thread is doing.
    guard input.start(onEmergencyStop: { run.cancel() }) else {
      blocked = request
      return
    }
    input.ignoreInput(for: Self.handsOffTime)
    self.run = run
    running = request
    result = nil
    state = RunState()
    stateURL = request.document.url
    run.setStepMode(on: request.stepByStep)
    run.setBreakpoints(blocks: Array(breakpoints[request.document.url] ?? []))
    hud = RunPanel(center: self)
    getOutOfTheWay()
    run.start(
      project: request.project.path, test: request.document.url.path,
      options: RunOptions(startAt: request.from, retries: nil),
      hands: MacHands(input: input, isCancelled: { run.isCancelled() }),
      secrets: StoredSecrets(), listener: RunBridge(center: self))
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
    var set = breakpoints[document.url] ?? []
    if set.remove(block) == nil { set.insert(block) }
    breakpoints[document.url] = set
    if running?.document.url == document.url { run?.setBreakpoints(blocks: Array(set)) }
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
    let declined = UserDefaults.standard.stringArray(forKey: Self.declinedKey) ?? []
    UserDefaults.standard.set(declined + [project.path], forKey: Self.declinedKey)
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
    let declined = UserDefaults.standard.stringArray(forKey: Self.declinedKey) ?? []
    let offers = !declined.contains(request.project.path) && GitIgnore.needsEntry(in: request.project)
    switch outcome {
    case .finished(_, let html, let report):
      let failure = report.attempts.last?.failure
      result = Result(
        test: request.document.url, project: request.project, verdict: report.verdict,
        message: RunText.outcome(report), report: html.map { URL(filePath: $0) },
        permission: failure.flatMap(RunText.permission), offersGitIgnore: offers)
    case .failed(let reason):
      result = Result(
        test: request.document.url, project: request.project, verdict: nil,
        message: String(localized: "The test couldn’t start: \(reason)"), report: nil, permission: nil,
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

/// Passes a run's progress from its thread to the main thread, in order.
private final class RunBridge: RunListener {
  let center: RunCenter

  init(center: RunCenter) {
    self.center = center
  }

  func event(event: RunEvent) {
    DispatchQueue.main.async { [center] in
      MainActor.assumeIsolated { center.apply(event) }
    }
  }

  func finished(outcome: RunOutcome) {
    DispatchQueue.main.async { [center] in
      MainActor.assumeIsolated { center.finish(outcome) }
    }
  }
}
