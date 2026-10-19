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
    /// Row of the data table the device chose, counted from 0, in place of the test's Run menu.
    var dataRow: Int? = nil
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
    /// Why messages about the run to Slack, Telegram or email didn't go.
    var notificationProblem: String?
    /// The run passed without any block looking at the screen.
    var nothingChecked = false
  }

  /// Tests run one after another, as Run All or a folder's Run starts them.
  struct Batch {
    /// "All Tests" or the folder's name.
    let title: String
    let project: URL
    /// Tests still to run.
    var queue: [URL]
    let total: Int
    var passed = 0
    var failed = 0
    /// Tests in quarantine that failed: they don't fail the batch.
    var quarantined = 0
    /// Tests that passed without checking anything.
    var unchecked = 0
    /// Tests that couldn't start: unreadable, with errors, or refused by the Mac.
    var notRun = 0
    var isStopping = false

    /// "Test 2 of 5": the test running now, counted from 1.
    var progress: String {
      String(localized: "Test \(total - queue.count) of \(total)")
    }
  }

  /// How the tests of a batch did, once it's over.
  struct BatchResult: Equatable {
    let title: String
    let total: Int
    let passed: Int
    let failed: Int
    /// Tests in quarantine that failed, not counted in `failed`.
    var quarantined = 0
    /// Tests that passed without checking anything, not counted in `passed`.
    var unchecked = 0
    /// The batch was stopped before every test ran.
    let stopped: Bool
  }

  /// The run that waits for a permission, a secret or a fix; shown as a sheet.
  var blocked: Request? {
    // Cancelling the sheet of a batch's test stops the batch.
    didSet { if blocked == nil, !isRunning, batch != nil { endBatch() } }
  }
  /// The test that runs now, while it runs.
  private(set) var running: Request?
  /// The current or last run, for the HUD and the canvas.
  private(set) var state: RunState?
  /// The test `state` belongs to.
  private(set) var stateURL: URL?
  private(set) var result: Result?
  /// The batch running now.
  private(set) var batch: Batch?
  private(set) var batchResult: BatchResult?
  /// Blocks to pause before, by test file. Not part of the test: kept in this Mac's settings.
  private(set) var breakpoints: [URL: Set<String>]

  @ObservationIgnored private var run: TestRun?
  @ObservationIgnored private let input = InputWatcher()
  @ObservationIgnored private var hud: GlassPanel?
  /// Whether JEVision is hidden for the run.
  @ObservationIgnored private var isOutOfTheWay = false

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
  /// `dataRow` runs only that row of the test's data table; `nil` runs every row.
  func runRemotely(_ test: String, in project: URL, dataRow: Int? = nil, requestedBy device: String) -> RemoteError? {
    guard !isRunning, blocked == nil else { return .RunInProgress }
    let request = Request(
      document: FlowDocument(url: project.appending(path: test)), project: project, from: nil, stepByStep: false,
      requestedBy: device, dataRow: dataRow)
    let missing = Self.missing(for: request)
    guard request.document.flow != nil, missing.errors == 0 else { return .TestUnreadable }
    guard missing.permissions.isEmpty, missing.values.isEmpty, start(request) else { return .HostFailed }
    return nil
  }

  /// Starts the run the sheet was holding back, now that nothing is missing.
  func confirm() {
    guard let request = blocked else { return }
    // Asks again, with a new sheet, if something is still missing.
    run(request.document, in: request.project, from: request.from, stepByStep: request.stepByStep)
    if isRunning { blocked = nil }
  }

  /// Runs `tests` one after another, each saved in the history, and sums up how they did.
  func runAll(_ tests: [URL], in project: URL, title: String) {
    guard !isRunning, blocked == nil, !tests.isEmpty else { return }
    batchResult = nil
    batch = Batch(title: title, project: project, queue: tests, total: tests.count)
    runNextInBatch()
  }

  func dismissBatchResult() { batchResult = nil }

  private func runNextInBatch() {
    while let current = batch, !current.isStopping, let test = current.queue.first {
      batch?.queue.removeFirst()
      run(FlowDocument(url: test), in: current.project)
      if isRunning { return }
      if blocked != nil {
        showApp()
        return
      }
      // Unreadable: `run` refuses it without a word.
      batch?.notRun += 1
    }
    endBatch()
  }

  private func endBatch() {
    guard let batch else { return }
    self.batch = nil
    let summary = BatchResult(
      title: batch.title, total: batch.total, passed: batch.passed, failed: batch.failed,
      quarantined: batch.quarantined, unchecked: batch.unchecked,
      stopped: batch.passed + batch.failed + batch.quarantined + batch.unchecked + batch.notRun < batch.total)
    result = nil
    batchResult = summary
    if isOutOfTheWay { showApp() }
    AccessibilityNotification.Announcement(RunText.batchOutcome(summary)).post()
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
    var choices = request.document.runChoices
    if let row = request.dataRow { choices.dataRow = row }
    let folder = request.document.url.deletingLastPathComponent()
    let table = choices.dataRow == nil ? nil : request.document.flow.flatMap { RunChoices.dataTable(of: $0, folder: folder) }
    run.start(
      project: request.project.path, test: request.document.url.path,
      options: RunOptions(
        startAt: request.from, retries: choices.retries.map(UInt32.init),
        environment: ProjectEnvironments.read(request.project).selected?.name, dataRows: choices.rows(in: table),
        vision: ai.settings(), localOnly: ai.localOnly,
        requestedBy: request.requestedBy, video: videos.mode, keepVideosDays: videos.keepDays),
      hands: MacHands(input: input, isCancelled: { run.isCancelled() }, clicking: clicking),
      secrets: StoredSecrets(), listener: RunBridge(center: self, project: request.project.path, remote: remoteListener?(request.project)))
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

  func stop() {
    batch?.isStopping = true
    run?.cancel()
  }

  /// Time to take the hand off the mouse after pressing Run, Resume or Step.
  private static let handsOffTime: TimeInterval = 1.5

  /// Brings the window back during a run, to look at the canvas.
  func showApp() {
    isOutOfTheWay = false
    NSApp.unhideWithoutActivation()
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

  /// How each block of `document` did in the run started from the app, and the block running
  /// now; `nil` when there's no such run since the test last changed.
  func marks(for document: FlowDocument) -> (marks: [String: RunState.Mark], current: String?)? {
    guard let state, stateURL == document.url else { return nil }
    return (state.marks, isRunning ? state.current : nil)
  }

  /// Ways out of the `if`s of `document` the run started from the app took; `nil` as for `marks`.
  func taken(for document: FlowDocument) -> Set<Slot>? {
    guard let state, stateURL == document.url else { return nil }
    return state.taken
  }

  /// Forgets the last run's marks once the test changes.
  func clearMarks(of document: FlowDocument) {
    guard !isRunning, stateURL == document.url else { return }
    state = nil
    stateURL = nil
  }

  func dismissResult() { result = nil }

  /// Messages about the run in `dir` didn't go; shown with its result while that is on screen.
  fileprivate func notificationsFailed(_ problem: String, run dir: String) {
    guard result?.run == URL(filePath: dir, directoryHint: .isDirectory) else { return }
    result?.notificationProblem = problem
  }

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
    if var batch {
      switch outcome {
      case .finished(_, _, let report) where report.nothingChecked: batch.unchecked += 1
      case .finished(_, _, let report) where report.verdict == .passed: batch.passed += 1
      case .finished(_, _, let report) where report.verdict == .failed && report.quarantined: batch.quarantined += 1
      case .finished(_, _, let report) where report.verdict == .failed: batch.failed += 1
      case .finished: batch.isStopping = true
      case .failed: batch.notRun += 1
      }
      self.batch = batch
      if !batch.isStopping, !batch.queue.isEmpty {
        // The window stays away: the next test starts right after this one.
        if case .finished(let dir, _, let report) = outcome {
          result = Result(
            test: request.document.url, project: request.project, verdict: report.verdict,
            message: RunText.outcome(report), run: URL(filePath: dir, directoryHint: .isDirectory),
            permission: nil, offersGitIgnore: false)
        }
        runNextInBatch()
        return
      }
      endBatch()
      return
    }
    showApp()
    let declined = defaults.stringArray(forKey: Self.declinedKey) ?? []
    let offers = !declined.contains(request.project.path) && GitIgnore.needsEntry(in: request.project)
    switch outcome {
    case .finished(let dir, _, let report):
      let failure = decidingAttempt(report: report)?.failure
      result = Result(
        test: request.document.url, project: request.project, verdict: report.verdict,
        message: RunText.outcome(report), run: URL(filePath: dir, directoryHint: .isDirectory),
        permission: failure.flatMap(RunText.permission), offersGitIgnore: offers,
        needsAISetup: failure.map(RunText.needsAISetup) ?? false, nothingChecked: report.nothingChecked)
    case .failed(let reason):
      result = Result(
        test: request.document.url, project: request.project, verdict: nil,
        message: String(localized: "The test couldn’t start: \(reason)"), run: nil, permission: nil,
        offersGitIgnore: false)
    }
    if let result { AccessibilityNotification.Announcement(result.message).post() }
  }

  // MARK: Windows

  /// Hides JEVision, as Hide in the app menu does, rather than putting its windows away: an app
  /// left without windows looks finished to window managers that quit such apps, and one did so
  /// mid-run. The run's panels can't be hidden, so they stay.
  private func getOutOfTheWay() {
    isOutOfTheWay = true
    NSApp.hide(nil)
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
  let project: String
  let remote: (any RunListener)?

  init(center: RunCenter, project: String, remote: (any RunListener)?) {
    self.center = center
    self.project = project
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
    // Still on the run's thread, which has nothing else to do: the services may take a while.
    guard case .finished(let dir, _, _) = outcome,
      let problem = NotificationsText.notify(project: project, dir: dir)
    else { return }
    DispatchQueue.main.async { [center] in
      MainActor.assumeIsolated { center.notificationsFailed(problem, run: dir) }
    }
  }
}
