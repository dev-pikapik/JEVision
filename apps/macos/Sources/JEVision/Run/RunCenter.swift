import AppKit
import JEVisionCore
import JEVisionHands
import Observation
import SwiftUI
import Synchronization

/// Runs tests from the app: checks that a run can start, gets the window out of the way, shows
/// the HUD while the tests drive the Mac, and brings the window back with the result. A batch
/// runs as many tests at once as its plan lets: side by side, only one of them on the screen.
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
    /// The test is in a batch only because other tests of it depend on it.
    var addedAsDependency = false
    /// Position of the test in the plan of its batch.
    var position: UInt32? = nil
  }

  /// A test running now.
  struct LiveRun {
    let request: Request
    fileprivate let run: TestRun
    /// The test acts on the screen or looks at it; the plan gives the screen to one such at a time.
    let usesScreen: Bool
    /// Paired phones follow this run.
    fileprivate let isFollowedFromAfar: Bool
    var state = RunState()
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

  /// Tests run as Run All or a folder's Run starts them, each after the tests it depends on.
  struct Batch {
    /// "All Tests" or the folder's name.
    let title: String
    let project: URL
    let plan: TestBatch
    /// The tests of the batch, chosen and added for their dependents, as the plan last said.
    var entries: [BatchEntry]
    /// Tests running now.
    var running = 0
    var passed = 0
    var failed = 0
    /// Tests in quarantine that failed: they don't fail the batch.
    var quarantined = 0
    /// Tests that passed without checking anything.
    var unchecked = 0
    /// Tests that couldn't start: unreadable, with errors, or refused by the Mac.
    var notRun = 0
    /// Tests that didn't run because a test they depend on didn't pass.
    var blocked = 0
    /// Blocked tests already saved in the history.
    var saved: Set<UInt32> = []
    /// The folders of the runs the batch made, blocked ones included, for the summary message.
    var runs: [String] = []
    var isStopping = false

    var total: Int { entries.count }
    /// Tests that ended one way or another.
    var done: Int { passed + failed + quarantined + unchecked + notRun + blocked }

    /// "Test 2 of 5": the test running now, counted from 1; "2 of 5 done · 2 running" while tests
    /// run side by side.
    var progress: String {
      running > 1
        ? String(localized: "\(done) of \(total) done · \(running) running")
        : String(localized: "Test \(min(done + 1, total)) of \(total)")
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
    /// Tests that didn't run because a test they depend on didn't pass.
    var blocked = 0
    /// Tests the batch added because chosen tests depend on them.
    var added = 0
    /// The batch was stopped before every test ran.
    let stopped: Bool
    /// Why the summary message to Slack, Telegram or email didn't go.
    var notificationProblem: String?
  }

  /// The run that waits for a permission, a secret or a fix; shown as a sheet.
  var blocked: Request? {
    // Cancelling the sheet of a batch's test stops the batch; the tests running end first.
    didSet {
      guard blocked == nil, let held = oldValue, let position = held.position,
        !live.contains(where: { $0.request.id == held.id })
      else { return }
      _ = batch?.plan.finish(position: position, outcome: .cancelled)
      stop()
      runNextInBatch()
    }
  }
  /// The tests running now, in the order they started.
  private(set) var live: [LiveRun] = []
  /// The run that ended last: the canvas keeps its marks until the test changes.
  private var last: LiveRun?
  /// The run the HUD and the canvas follow: one that waits for a person, else the one on the
  /// screen, else the first started.
  private var shown: LiveRun? {
    live.first { $0.state.isPaused } ?? live.first(where: \.usesScreen) ?? live.first
  }
  /// The test the HUD shows, while tests run.
  var running: Request? { shown?.request }
  /// The run the HUD shows, or the last one, for the HUD and the canvas.
  var state: RunState? { (shown ?? last)?.state }
  private(set) var result: Result?
  /// The batch running now.
  private(set) var batch: Batch?
  private(set) var batchResult: BatchResult?
  /// Blocks to pause before, by test file. Not part of the test: kept in this Mac's settings.
  private(set) var breakpoints: [URL: Set<String>]

  /// The runs going on, for Esc Esc to cancel right on the input watcher's thread.
  private nonisolated let cancellable = Mutex<[TestRun]>([])
  /// Counts the batches that ended, so a late answer from the services finds its own result.
  @ObservationIgnored private var endedBatches = 0
  @ObservationIgnored private let input: InputWatcher
  @ObservationIgnored private var hud: GlassPanel?
  /// Whether JEVision is hidden for the runs.
  @ObservationIgnored private var isOutOfTheWay = false

  @ObservationIgnored private let defaults: UserDefaults
  /// Also hears every run of the project at the URL, for remote access.
  @ObservationIgnored var remoteListener: ((URL) -> (any RunListener)?)?
  /// Starts a run on this Mac when nothing keeps it back, telling the listener how it goes;
  /// `false` when something does. Tests pass runs that touch nothing.
  typealias Starter = @MainActor (Request, TestRun, any RunListener) -> Bool
  @ObservationIgnored private let starts: Starter?
  /// Makes the run of each test; tests watch what the HUD's buttons do to it.
  @ObservationIgnored private let makeRun: () -> TestRun

  private static let declinedKey = "gitIgnoreDeclined"
  private static let breakpointsKey = "breakpoints"

  init(defaults: UserDefaults = .standard, starts: Starter? = nil, makeRun: @escaping () -> TestRun = { TestRun() }) {
    self.defaults = defaults
    self.starts = starts
    self.makeRun = makeRun
    // The watcher reads on its own thread; reading defaults is safe from any.
    nonisolated(unsafe) let store = defaults
    input = InputWatcher(settings: { RunInputSettings(store) })
    let saved = defaults.dictionary(forKey: Self.breakpointsKey) as? [String: [String]] ?? [:]
    breakpoints = Dictionary(uniqueKeysWithValues: saved.map { (URL(filePath: $0.key), Set($0.value)) })
  }

  var isRunning: Bool { !live.isEmpty }
  /// Tests run, a batch goes on or a sheet waits for a person: another run can't start now.
  var isBusy: Bool { isRunning || blocked != nil || batch != nil }

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
    guard !isBusy, document.flow != nil else { return }
    let request = Request(document: document, project: project, from: block, stepByStep: stepByStep)
    if !start(request) { blocked = request }
  }

  /// Runs `test`, relative to `project`, for a paired device, or says why it can't. Nothing
  /// waits for a person here: what a local run would ask for refuses the remote one.
  /// `dataRow` runs only that row of the test's data table; `nil` runs every row.
  func runRemotely(_ test: String, in project: URL, dataRow: Int? = nil, requestedBy device: String) -> RemoteError? {
    guard !isBusy else { return .RunInProgress }
    let request = Request(
      document: FlowDocument(url: project.appending(path: test), project: project), project: project, from: nil, stepByStep: false,
      requestedBy: device, dataRow: dataRow)
    let missing = Self.missing(for: request)
    guard request.document.flow != nil, missing.errors == 0 else { return .TestUnreadable }
    guard start(request) else { return .HostFailed }
    return nil
  }

  /// Starts the run the sheet was holding back, now that nothing is missing, and lets the batch
  /// it belongs to go on. The sheet stays while something still is.
  func confirm() {
    guard let request = blocked, start(request, usesScreen: usesScreen(request)) else { return }
    blocked = nil
    runNextInBatch()
  }

  /// Runs `tests` with the tests they depend on, each after its dependencies and saved in the
  /// history, and sums up how they did. A test whose dependency didn't pass is saved as blocked
  /// instead of run. They run as the project settings say for `folder`, relative to the project.
  func runAll(_ tests: [URL], in project: URL, title: String, folder: String = ".") {
    guard !isBusy, !tests.isEmpty else { return }
    batchResult = nil
    let path = project.path(percentEncoded: false)
    // Settings that can't be read fail each run with the reason; one after another is the safe way.
    let mode = (try? projectSettings(project: path)).map { folderRunMode(settings: $0, folder: folder) } ?? .sequential
    let plan = TestBatch(project: path, tests: tests.map { RelativePath.from(project, to: $0) }, mode: mode)
    batch = Batch(title: title, project: project, plan: plan, entries: plan.entries())
    runNextInBatch()
  }

  func dismissBatchResult() { batchResult = nil }

  /// Starts every test of the batch its plan lets start now, and ends the batch once none runs and
  /// none can start. A test that can't start holds back the ones after it until its sheet is answered.
  private func runNextInBatch() {
    while var current = batch {
      saveBlocked(&current)
      guard !current.isStopping, blocked == nil, let next = current.plan.startNext() else {
        batch = current
        break
      }
      current.entries = current.plan.entries()
      batch = current
      let entry = current.entries[Int(next)]
      let document = FlowDocument(url: current.project.appending(path: entry.file), project: current.project)
      // Unreadable: it fails without a word.
      guard document.flow != nil else {
        batch?.notRun += 1
        _ = batch?.plan.finish(position: next, outcome: .notRun)
        continue
      }
      let request = Request(
        document: document, project: current.project, from: nil, stepByStep: false, addedAsDependency: entry.added,
        position: next)
      if !start(request, usesScreen: entry.usesScreen) {
        blocked = request
        // While a test uses the screen, the window would get in its way; the sheet shows once it is done.
        if !live.contains(where: \.usesScreen) { showApp() }
      }
    }
    guard live.isEmpty else { return }
    putAway()
    if blocked == nil { endBatch() }
  }

  /// Whether the test of `request` uses the screen, as its batch's plan says; a test run on its
  /// own counts as one that does.
  private func usesScreen(_ request: Request) -> Bool {
    guard let position = request.position, let entries = batch?.entries, entries.indices.contains(Int(position)) else {
      return true
    }
    return entries[Int(position)].usesScreen
  }

  /// Saves the tests the plan blocked since the last look, so the history shows why they didn't run.
  private func saveBlocked(_ batch: inout Batch) {
    batch.entries = batch.plan.entries()
    let environment = ProjectEnvironments.read(batch.project).selected?.name
    for (index, entry) in batch.entries.enumerated() {
      guard case .blocked(let blocker) = entry.state, batch.saved.insert(UInt32(index)).inserted else { continue }
      batch.blocked += 1
      let saved = saveBlockedRun(
        project: batch.project.path(percentEncoded: false), test: entry.file, blocker: blocker,
        options: RunOptions(
          startAt: nil, retries: nil, environment: environment, vision: nil, localOnly: false, video: .off,
          addedAsDependency: entry.added, language: AppLanguage.shared.language))
      if case .finished(let dir, _, _) = saved { batch.runs.append(dir) }
    }
  }

  private func endBatch() {
    guard var batch else { return }
    batch.plan.stop()
    saveBlocked(&batch)
    self.batch = nil
    let summary = BatchResult(
      title: batch.title, total: batch.total, passed: batch.passed, failed: batch.failed,
      quarantined: batch.quarantined, unchecked: batch.unchecked, blocked: batch.blocked,
      added: batch.entries.count { $0.added }, stopped: batch.done < batch.total)
    result = nil
    batchResult = summary
    if isOutOfTheWay { showApp() }
    AccessibilityNotification.Announcement(RunText.batchOutcome(summary)).post()
    notify(about: batch, summary: summary)
  }

  /// One summary message about the batch, as the project's notification settings ask. A stopped
  /// batch sends nothing. The services may take a while, so it goes out in the background.
  private func notify(about batch: Batch, summary: BatchResult) {
    endedBatches += 1
    guard !summary.stopped, !batch.isStopping, !batch.runs.isEmpty else { return }
    let (project, dirs, serial) = (batch.project.path(percentEncoded: false), batch.runs, endedBatches)
    Task.detached { [weak self] in
      guard let problem = NotificationsText.notify(project: project, dirs: dirs) else { return }
      await self?.batchNotificationsFailed(problem, batch: serial)
    }
  }

  /// The summary message of a batch didn't go; shown with its result while that is on screen.
  private func batchNotificationsFailed(_ problem: String, batch serial: Int) {
    guard serial == endedBatches else { return }
    batchResult?.notificationProblem = problem
  }

  /// Starts the run; `false` when something it needs is missing or the emergency stop can't be armed.
  private func start(_ request: Request, usesScreen: Bool = true) -> Bool {
    let run = makeRun()
    run.setStepMode(on: request.stepByStep)
    // Breakpoints are for debugging here; a run from afar would wait at them for nobody.
    if request.requestedBy == nil {
      run.setBreakpoints(blocks: Array(breakpoints[request.document.url] ?? []))
    }
    // The phones follow one run at a time: the core keeps one live run for them, so a test that
    // starts beside the one they follow is only in their history.
    let remote = live.contains(where: \.isFollowedFromAfar) ? nil : remoteListener?(request.project)
    let listener = RunBridge(
      center: self, run: request.id, project: request.project.path, remote: remote, notifies: batch == nil)
    guard starts?(request, run, listener) ?? drive(request, run, listener, usesScreen: usesScreen) else { return false }
    live.append(LiveRun(request: request, run: run, usesScreen: usesScreen, isFollowedFromAfar: remote != nil))
    cancellable.withLock { $0.append(run) }
    if request.position != nil { batch?.running += 1 }
    AppLog.runStarted(request.document.url, project: request.project)
    result = nil
    return true
  }

  /// Lets the test drive this Mac, once nothing it needs is missing. The first run arms Esc Esc,
  /// which stops every run, and opens the HUD; both stay until the last run ends.
  private func drive(_ request: Request, _ run: TestRun, _ listener: any RunListener, usesScreen: Bool) -> Bool {
    let missing = Self.missing(for: request)
    guard missing.permissions.isEmpty, missing.values.isEmpty, missing.errors == 0 else { return false }
    let ai = AISettings(defaults)
    let videos = VideoSettings(defaults)
    let isFirst = hud == nil
    if isFirst {
      guard input.start(onEmergencyStop: { [weak self] in self?.emergencyStop() }) else { return false }
      hud = GlassPanel(RunHUD(center: self), width: RunHUD.width, height: RunHUD.height)
    }
    input.ignoreInput(for: Self.handsOffTime)
    // A test on the screen needs the window away, even if it was brought back to look at the canvas.
    if isFirst || usesScreen { getOutOfTheWay() }
    let hud = hud
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
        requestedBy: request.requestedBy, fallback: ai.fallback(), providers: ai.namedProviders, usage: ai.usage, video: videos.mode,
        keepVideosDays: videos.keepDays, addedAsDependency: request.addedAsDependency, language: AppLanguage.shared.language),
      hands: MacHands(input: input, isCancelled: { run.isCancelled() }, clicking: clicking),
      secrets: StoredSecrets(), listener: listener)
    return true
  }

  // MARK: Control

  // Pause, Resume and Step are for the run the HUD shows; Stop is for all of them.

  func pause() { shown?.run.pause() }

  func resume() {
    input.ignoreInput(for: Self.handsOffTime)
    getOutOfTheWay()
    shown?.run.resume()
  }

  func step() {
    input.ignoreInput(for: Self.handsOffTime)
    getOutOfTheWay()
    shown?.run.step()
  }

  func stop() {
    batch?.isStopping = true
    for live in live { live.run.cancel() }
  }

  /// Esc Esc: cancels the runs right on the input watcher's thread, whatever the main thread is
  /// doing, then stops them as Stop does.
  nonisolated func emergencyStop() {
    cancellable.withLock { $0 }.forEach { $0.cancel() }
    DispatchQueue.main.async { MainActor.assumeIsolated { self.stop() } }
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
    for live in live where live.request.document.url == document.url && live.request.requestedBy == nil {
      live.run.setBreakpoints(blocks: Array(set))
    }
    let saved = Dictionary(uniqueKeysWithValues: breakpoints.map { ($0.key.path, $0.value.sorted()) })
    defaults.set(saved, forKey: Self.breakpointsKey)
  }

  // MARK: Results

  /// How each block of `document` did in its run started from the app, and the block running
  /// now; `nil` when there's no such run since the test last changed. A run of the test going on,
  /// shown in the HUD or not, comes before the one that ended last.
  func marks(for document: FlowDocument) -> (marks: [String: RunState.Mark], current: String?)? {
    if let live = live.first(where: { $0.request.document.url == document.url }) {
      return (live.state.marks, live.state.current)
    }
    guard let last, last.request.document.url == document.url else { return nil }
    return (last.state.marks, nil)
  }

  /// Ways out of the `if`s of `document` the run started from the app took; `nil` as for `marks`.
  func taken(for document: FlowDocument) -> Set<Slot>? {
    let run = live.first { $0.request.document.url == document.url } ?? last
    return run?.request.document.url == document.url ? run?.state.taken : nil
  }

  /// Forgets the last run's marks once the test changes.
  func clearMarks(of document: FlowDocument) {
    if last?.request.document.url == document.url { last = nil }
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

  fileprivate func apply(_ event: RunEvent, to id: Request.ID) {
    guard let index = live.firstIndex(where: { $0.request.id == id }) else { return }
    if case .stepFinished(_, let step) = event { AppLog.step(step) }
    let wasPaused = live[index].state.isPaused
    let wasWaiting = live[index].state.isWaitingForScreen
    live[index].state.apply(event)
    let state = live[index].state
    // A run that stops for a person takes the HUD, so the announcement is about what it shows.
    if state.isPaused, !wasPaused {
      AccessibilityNotification.Announcement(RunHUD.status(of: state)).post()
    }
    if case .waitingForScreen(let holder) = state.phase, !wasWaiting {
      AccessibilityNotification.Announcement(RunText.screenBusy(holder)).post()
    }
  }

  fileprivate func finish(_ outcome: RunOutcome, of id: Request.ID) {
    guard let index = live.firstIndex(where: { $0.request.id == id }) else { return }
    let ended = live.remove(at: index)
    last = ended
    cancellable.withLock { $0.removeAll { $0 === ended.run } }
    let request = ended.request
    AppLog.runEnded(request.document.url, project: request.project, outcome: outcome)
    if var batch, let position = request.position {
      batch.running -= 1
      if case .finished(let dir, _, _) = outcome { batch.runs.append(dir) }
      let verdict: TestOutcome
      switch outcome {
      // It did all it was asked to without failing, so the tests after it can go on.
      case .finished(_, _, let report) where report.nothingChecked:
        batch.unchecked += 1
        verdict = .passed
      case .finished(_, _, let report) where report.verdict == .passed:
        batch.passed += 1
        verdict = .passed
      case .finished(_, _, let report) where report.verdict == .failed && report.quarantined:
        batch.quarantined += 1
        verdict = .failed
      case .finished(_, _, let report) where report.verdict == .failed:
        batch.failed += 1
        verdict = .failed
      case .finished:
        batch.isStopping = true
        verdict = .cancelled
      case .failed:
        batch.notRun += 1
        verdict = .notRun
      }
      _ = batch.plan.finish(position: position, outcome: verdict)
      self.batch = batch
      // A cancelled test stops the batch, the tests beside it too.
      if batch.isStopping { stop() }
      // The window stays away: the next tests start right after this one.
      if !batch.isStopping, case .finished(let dir, _, let report) = outcome {
        result = Result(
          test: request.document.url, project: request.project, verdict: report.verdict,
          message: RunText.outcome(report), run: URL(filePath: dir, directoryHint: .isDirectory),
          permission: nil, offersGitIgnore: false)
      }
      runNextInBatch()
      // The sheet of a test that couldn't start waited for the screen to be free.
      if blocked != nil, ended.usesScreen, !live.contains(where: \.usesScreen) { showApp() }
      return
    }
    guard live.isEmpty else { return }
    putAway()
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
  /// mid-run. The run's panels can't be hidden, so they stay. Settings can keep the window where it
  /// is; the panel shows either way.
  private func getOutOfTheWay() {
    if RunInputSettings(defaults).collapsesToPanel {
      isOutOfTheWay = true
      NSApp.hide(nil)
    }
    hud?.show()
  }

  /// Puts away what the runs needed once none is left: Esc Esc stops nothing now, and the HUD closes.
  private func putAway() {
    input.stop()
    hud?.close()
    hud = nil
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
  let run: RunCenter.Request.ID
  let project: String
  let remote: (any RunListener)?
  /// Tells the project's channels about the run when it ends; a batch tells about all its runs at once.
  let notifies: Bool

  init(center: RunCenter, run: RunCenter.Request.ID, project: String, remote: (any RunListener)?, notifies: Bool) {
    self.center = center
    self.run = run
    self.project = project
    self.remote = remote
    self.notifies = notifies
  }

  func event(event: RunEvent) {
    remote?.event(event: event)
    DispatchQueue.main.async { [center, run] in
      MainActor.assumeIsolated { center.apply(event, to: run) }
    }
  }

  func finished(outcome: RunOutcome) {
    remote?.finished(outcome: outcome)
    DispatchQueue.main.async { [center, run] in
      MainActor.assumeIsolated { center.finish(outcome, of: run) }
    }
    // Still on the run's thread, which has nothing else to do: the services may take a while.
    guard notifies, case .finished(let dir, _, _) = outcome,
      let problem = NotificationsText.notify(project: project, dirs: [dir])
    else { return }
    DispatchQueue.main.async { [center] in
      MainActor.assumeIsolated { center.notificationsFailed(problem, run: dir) }
    }
  }
}
