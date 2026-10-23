import Foundation
import JEVisionCore
import JEVisionHands

/// `jevision run`: runs tests on this Mac, one after another or side by side, the tests they depend on first, shows how it goes and saves reports.
struct RunCommand {
  let request: RunRequest
  /// Progress and the summary; standard error when the result goes out as JSON.
  var progress: Console
  /// The summary, or the result as JSON.
  var output = Console.standardOutput

  init(request: RunRequest) {
    self.request = request
    progress = request.json ? .standardError : .standardOutput
  }

  func execute() -> ExitStatus {
    do {
      return finish(try runTests())
    } catch {
      Console.standardError.line(error.message)
      if request.json { output.line(JSON.result([("error", error.json)]).pretty) }
      return error.status
    }
  }

  /// Runs the tests one after another; the error is what to tell when they can't start.
  func runTests() throws(UsageError) -> [TestResult] {
    let plan: (project: String, tests: [String], environment: String?, filteredOut: Int, folder: String)
    do {
      plan = try Self.plan(request)
    } catch {
      throw UsageError(message: "jevision: \(error.message)", code: error.code, status: error.status)
    }
    if plan.filteredOut > 0 {
      progress.line(progress.paint(words("cli.run.filteredOut", plan.filteredOut + plan.tests.count, plan.filteredOut), .dim))
    }
    let ai = AISettings.app
    let vision = ai.settings(provider: request.provider, model: request.model)
    if request.model != nil, vision == nil {
      throw UsageError(message: "jevision: " + words("cli.error.modelNeedsProvider"), code: "modelNeedsProvider")
    }
    if let problem = Readiness.problem() {
      throw UsageError(message: problem, code: "notReady", status: .notReady)
    }
    let input = InputWatcher()
    let videos = VideoSettings.app
    var request = request
    request.environment = plan.environment
    request.video = request.video ?? videos.mode
    let session = TestSession(
      project: plan.project, tests: plan.tests, folder: plan.folder, request: request, progress: progress, input: input,
      vision: vision, fallback: vision.flatMap { ai.fallback(besides: $0.provider) }, providers: ai.namedProviders,
      usage: ai.usage,
      localOnly: ai.localOnly, keepVideosDays: videos.keepDays
    ) { run in
      MacHands(input: input, isCancelled: { run.isCancelled() })
    }
    // Esc Esc stops the run right on the watcher's thread.
    guard input.start(onEmergencyStop: { [session] in session.stop() }) else {
      throw UsageError(message: "jevision: " + words("cli.run.noKeyboardWatch"), code: "noKeyboardWatch", status: .notReady)
    }
    defer { input.stop() }
    let interrupts = Interrupts.install { [session, progress] in
      session.stop()
      progress.line(progress.paint(words("cli.run.pressAgain"), .dim))
    }
    return withExtendedLifetime(interrupts) { session.runAll() }
  }

  // MARK: Planning

  /// The project folder and the test files to run, as real absolute paths, and the environment
  /// to run in: the one asked for, otherwise the default of the project's environments file.
  /// `filteredOut` counts the named tests the request's filter leaves out. `folder`, relative to
  /// the project, is the deepest one that holds the files and folders named: its settings choose
  /// how the tests run together.
  static func plan(_ request: RunRequest) throws(UsageError) -> (
    project: String, tests: [String], environment: String?, filteredOut: Int, folder: String
  ) {
    var targets: [String] = []
    for path in request.paths {
      // The app lists tests without the suffix, so `notes/add-milk` runs notes/add-milk.flow.json.
      guard let real = realPath(path) ?? realPath(path + ".flow.json") else {
        throw UsageError(message: words("cli.error.noFileOrFolder", path), code: "noFileOrFolder")
      }
      targets.append(real)
    }
    var (tests, _) = collectTests(targets)
    guard !tests.isEmpty else {
      throw UsageError(message: words("cli.error.noTests", request.paths.joined(separator: ", ")), code: "noTests")
    }
    let project: String
    if let given = request.project {
      guard let real = realPath(given), isFolder(real) else {
        throw UsageError(message: words("cli.error.noFolder", given), code: "noFolder")
      }
      project = real
    } else {
      project = defaultProject(for: targets[0], tests: tests)
    }
    if let outside = tests.first(where: { !isInside($0, project) }) {
      throw UsageError(message: words("cli.error.outsideProject", outside, project), code: "outsideProject")
    }
    let named = tests.count
    tests = try chosen(tests, by: request.filter, in: project)
    let folder = sharedFolder(
      folders: targets.map { target in
        let folder = isFolder(target) ? target : (target as NSString).deletingLastPathComponent
        return isInside(folder, project) ? String(folder.dropFirst(project == "/" ? 1 : project.count + 1)) : "."
      })
    guard let environment = request.environment else {
      return (
        project, tests, try readEnvironments(in: project) { try defaultEnvironment(project: project) }, named - tests.count, folder
      )
    }
    try checkEnvironment(environment, in: project)
    return (project, tests, environment, named - tests.count, folder)
  }

  /// The `tests` whose cards `filter` chooses; all of them for a filter that asks nothing. Flaky
  /// counts as in the app: marked in the card, or found from the runs saved in `project`.
  static func chosen(_ tests: [String], by filter: TestFilter, in project: String) throws(UsageError) -> [String] {
    guard !testFilterIsEmpty(filter: filter) else { return tests }
    var flaky: Set<String> = []
    if filter.flaky, let runs = try? listRuns(project: project), !runs.isEmpty {
      flaky = Set(flakyTests(runs: runs.map(\.report), nowMs: Int64(Date.now.timeIntervalSince1970 * 1000)).keys)
    }
    let cards = tests.map { file -> TestCard in
      var card = (try? String(contentsOfFile: file, encoding: .utf8)).map { testSummary(json: $0).card } ?? TestCard()
      let relative = file.hasPrefix(project + "/") ? String(file.dropFirst(project.count + 1)) : file
      if flaky.contains(relative) { card.flaky = true }
      return card
    }
    let kept = filterTests(filter: filter, cards: cards).map { tests[Int($0)] }
    guard !kept.isEmpty else {
      throw UsageError(message: words("cli.error.noTestFits", tests.count), code: "noTestFits")
    }
    return kept
  }

  /// Fails unless the project has the environment `name`, saying which ones it has.
  private static func checkEnvironment(_ name: String, in project: String) throws(UsageError) {
    let file = environmentsFileName()
    let all = try readEnvironments(in: project) { try projectEnvironments(project: project) }
    guard !all.contains(where: { $0.name == name }) else { return }
    if all.isEmpty {
      throw UsageError(message: words("cli.error.noEnvironmentsFile", name, file, project), code: "noEnvironmentsFile")
    }
    throw UsageError(
      message: words("cli.error.noEnvironment", name, file, all.map(\.name).joined(separator: ", ")), code: "noEnvironment")
  }

  /// What `read` gets from the environments file of `project`, or why the file can't be read.
  private static func readEnvironments<T>(in project: String, _ read: () throws -> T) throws(UsageError) -> T {
    let file = environmentsFileName()
    let status = ExitStatus.unreadable((project as NSString).appendingPathComponent(file))
    do {
      return try read()
    } catch EnvironmentsError.Unreadable(_, let reason) {
      throw UsageError(
        message: words("cli.error.environmentsUnreadable", file, project, reason), code: "environmentsUnreadable", status: status)
    } catch {
      throw UsageError(
        message: words("cli.error.environmentsUnreadable", file, project, error.localizedDescription),
        code: "environmentsUnreadable", status: status)
    }
  }

  /// The nearest folder above `target` that has a `.jevision` folder, as the app makes one in
  /// the project it opens, or the environments file; otherwise the current folder if it holds all the tests; otherwise
  /// the folder of `target`.
  static func defaultProject(for target: String, tests: [String]) -> String {
    let start = isFolder(target) ? target : (target as NSString).deletingLastPathComponent
    if let project = enclosingProject(start) { return project }
    // Not `/`, where launchd starts programs: reports can't be saved there.
    if let current = realPath(FileManager.default.currentDirectoryPath), current != "/",
      tests.allSatisfy({ isInside($0, current) })
    {
      return current
    }
    return start
  }

  private static func isInside(_ path: String, _ folder: String) -> Bool {
    folder == "/" || path.hasPrefix(folder + "/")
  }

  static func realPath(_ path: String) -> String? {
    guard let resolved = realpath(path, nil) else { return nil }
    defer { free(resolved) }
    return String(cString: resolved)
  }

  // MARK: Results

  /// Saves the JUnit and HTML reports asked for and prints the summary or the JSON result.
  func finish(_ results: [TestResult]) -> ExitStatus {
    var status =
      results.map { request.includeQuarantinedFailures && $0.verdict == "quarantined" ? .problems : $0.status }.max() ?? .ok
    let runs = results.compactMap(\.dir)
    // A report that can't be saved is told on standard error; the JSON result tells the first.
    var problem: UsageError?
    if let junit = request.junit {
      do {
        try exportJunit(dirs: runs, to: junit, includeQuarantinedFailures: request.includeQuarantinedFailures)
        progress.line(progress.paint(words("cli.run.junitReport", junit), .dim))
      } catch {
        let error = UsageError(message: words("cli.error.cannotWrite", junit, "\(error)"), code: "cannotWrite", status: .io)
        Console.standardError.line(error.message)
        problem = error
        status = max(status, .io)
      }
    }
    if let folder = request.html {
      do {
        try FileManager.default.createDirectory(atPath: folder, withIntermediateDirectories: true)
        for dir in runs {
          try exportHtml(dir: dir, to: (folder as NSString).appendingPathComponent((dir as NSString).lastPathComponent + ".html"))
        }
        if !runs.isEmpty { progress.line(progress.paint(words("cli.run.htmlReports", folder), .dim)) }
      } catch {
        let error = UsageError(message: words("cli.error.cannotWrite", folder, "\(error)"), code: "cannotWrite", status: .io)
        Console.standardError.line(error.message)
        problem = problem ?? error
        status = max(status, .io)
      }
    }
    if request.json { output.line(Self.json(results, error: problem).pretty) }
    return status
  }

  static func json(_ results: [TestResult], error: UsageError? = nil) -> JSON {
    let count = { (verdict: String) in JSON.int(Int64(results.count { $0.verdict == verdict })) }
    return .result([
      ("passed", count("passed")),
      ("nothingChecked", count("nothingChecked")),
      ("failed", count("failed")),
      ("quarantined", count("quarantined")),
      ("cancelled", count("cancelled")),
      ("blocked", count("blocked")),
      ("notRun", count("notRun")),
      ("errors", count("error")),
      ("tests", .array(results.map(\.json))),
    ] + (error.map { [("error", $0.json)] } ?? []))
  }
}

/// One test of a `jevision run` and how it went.
struct TestResult {
  enum Outcome {
    case finished(dir: String, html: String?, report: RunReport)
    /// The run couldn't save its report.
    case error(String)
    /// The run was stopped before this test.
    case notRun
  }

  /// Relative to the project folder.
  let file: String
  let outcome: Outcome

  var dir: String? {
    if case .finished(let dir, _, _) = outcome { dir } else { nil }
  }

  var verdict: String {
    switch outcome {
    case .finished(_, _, let report):
      switch report.verdict {
      case .passed: report.nothingChecked ? "nothingChecked" : "passed"
      case .failed: report.quarantined ? "quarantined" : "failed"
      case .cancelled: "cancelled"
      case .blocked: "blocked"
      case .unknown: "unknown"
      }
    case .error: "error"
    case .notRun: "notRun"
    }
  }

  /// A failure in quarantine doesn't count unless asked: see `RunRequest.includeQuarantinedFailures`.
  var status: ExitStatus {
    switch verdict {
    case "passed", "quarantined", "nothingChecked": .ok
    case "error": .io
    default: needsNewer ? .needsNewer : .problems
    }
  }

  /// The test or a block it uses was saved by a newer JEVision.
  private var needsNewer: Bool {
    guard case .finished(_, _, let report) = outcome, let code = decidingAttempt(report: report)?.failure?.code else { return false }
    return asksForNewerJevision(code: code)
  }

  var json: JSON {
    var members: [(String, JSON)] = [("file", .string(file)), ("verdict", .string(verdict))]
    switch outcome {
    case .finished(let dir, let html, let report):
      let failure = decidingAttempt(report: report)?.failure
      members += [
        ("name", .string(report.test)),
        ("flaky", .bool(report.flaky)),
        ("quarantined", .bool(report.quarantined)),
        ("quarantineReason", report.quarantineReason.map { .string($0) } ?? .null),
        ("blockedBy", report.blocked.map(\.json) ?? .null),
        ("addedAsDependency", .bool(report.addedAsDependency)),
        ("attempts", .int(Int64(report.attempts.count))),
        ("rows", .array(report.rows.map(\.json))),
        ("durationMs", .int(Int64(report.durationMs))),
        ("newReferences", .array(report.baselinesCreated.map { .string($0) })),
        ("failure", failure.map(\.json) ?? .null),
        ("report", .string((dir as NSString).appendingPathComponent("report.json"))),
        ("html", .string(html)),
        ("video", report.video.map { .string((dir as NSString).appendingPathComponent($0)) } ?? .null),
      ]
    case .error(let reason):
      members.append(("error", .string(reason)))
    case .notRun:
      break
    }
    return .object(members)
  }
}

/// Runs the tests one at a time, or side by side as `--parallel` or the project settings for
/// their folder ask and the core's plan allows; `stop` cancels the ones running and skips the rest.
final class TestSession: @unchecked Sendable {
  private let project: String
  private let tests: [String]
  /// Relative to the project: the folder whose settings choose how the tests run together.
  private let folder: String
  private let request: RunRequest
  private let progress: Console
  private let makeHands: (TestRun) -> any Hands
  private let vision: VisionSettings?
  private let fallback: VisionSettings?
  private let providers: ProviderSettings?
  private let usage: AiUsageSettings?
  private let localOnly: Bool
  private let keepVideosDays: UInt32?
  /// The person's mouse and keyboard, watched while a test runs.
  private let input: InputWatcher?
  private let lock = NSLock()
  /// The tests running now, by position in the plan.
  private var running: [Int: TestRun] = [:]
  private var outcomes: [TestResult.Outcome?] = []
  private var stopped = false
  /// A test ended: the plan may have more to start.
  private let ended = DispatchSemaphore(value: 0)
  /// Keeps what goes to the console in pieces that stay whole.
  private let printing = NSLock()

  init(
    project: String, tests: [String], folder: String = ".", request: RunRequest, progress: Console, input: InputWatcher?,
    vision: VisionSettings? = nil, fallback: VisionSettings? = nil, providers: ProviderSettings? = nil,
    usage: AiUsageSettings? = nil,
    localOnly: Bool = false, keepVideosDays: UInt32? = nil, makeHands: @escaping (TestRun) -> any Hands
  ) {
    self.vision = vision
    self.fallback = fallback
    self.providers = providers
    self.usage = usage
    self.localOnly = localOnly
    self.keepVideosDays = keepVideosDays
    self.project = project
    self.tests = tests
    self.folder = folder
    self.request = request
    self.progress = progress
    self.input = input
    self.makeHands = makeHands
  }

  /// Stops the tests that are running, which are then reported as cancelled, and the ones after them.
  func stop() {
    let (runs, wasStopped) = lock.withLock {
      defer { stopped = true }
      return (Array(running.values), stopped)
    }
    runs.forEach { $0.cancel() }
    guard !wasStopped else { return }
    emit("\n" + progress.paint(words("cli.run.stopping"), .yellow) + "\n")
  }

  /// How the tests run together: as `--parallel` asks, otherwise as the project settings say for
  /// the folder of the run, with the words that tell so; one at a time, with a warning, when the
  /// settings can't be read.
  private func runMode() -> (RunMode, String?) {
    if let limit = request.parallel { return (.parallel(limit: limit), nil) }
    guard tests.count > 1 else { return (.sequential, nil) }
    let file = projectSettingsFileName()
    let settings: ProjectSettings
    do {
      settings = try projectSettings(project: project)
    } catch {
      let reason = if case ProjectSettingsError.Unreadable(_, let reason) = error { reason } else { error.localizedDescription }
      emit(progress.paint(words("cli.run.settingsUnreadable", file, reason), .yellow) + "\n")
      return (.sequential, nil)
    }
    guard let setting = folderRunModeSetting(settings: settings, folder: folder) else { return (.sequential, nil) }
    let source =
      setting.folder == "."
      ? words("cli.run.modeForProject", file) : words("cli.run.modeForFolder", file, setting.folder)
    return (setting.mode, source)
  }

  /// Writes `text` in one piece, so it doesn't mix with what another test prints.
  private func emit(_ text: String) {
    printing.withLock { progress.write(text) }
  }

  func runAll() -> [TestResult] {
    let started = Date()
    let (mode, chosenBy) = runMode()
    let batch = TestBatch(
      project: project, tests: tests.map { String($0.dropFirst(project == "/" ? 1 : project.count + 1)) }, mode: mode)
    let entries = batch.entries()
    // Tests that use the screen take turns, so side by side only pays when one of them leaves it alone.
    let limit: UInt32 = if case .parallel(let limit) = mode { limit } else { 1 }
    let sideBySide = limit > 1 && entries.count > 1 && entries.contains { !$0.usesScreen }
    emit(
      (sideBySide
        ? words("cli.run.startingSideBySide", tests.count, project, limit) : words("cli.run.starting", tests.count, project)) + "\n")
    if sideBySide, let chosenBy {
      emit(progress.paint(chosenBy, .dim) + "\n")
    }
    if limit > 1, entries.count > 1, !sideBySide {
      emit(progress.paint(words("cli.run.screenOneAtATime"), .dim) + "\n")
    }
    let added = entries.filter(\.added).map(\.name)
    if !added.isEmpty {
      emit(progress.paint(words("cli.run.addedDependencies", added.joined(separator: ", ")), .dim) + "\n")
    }
    lock.withLock { outcomes = Array(repeating: nil, count: entries.count) }
    func saveBlockedTests() {
      for (i, entry) in batch.entries().enumerated() where lock.withLock({ outcomes[i] == nil }) {
        if case .blocked(let blocker) = entry.state {
          let outcome = saveBlocked(entry, blocker)
          lock.withLock { outcomes[i] = outcome }
        }
      }
    }
    while true {
      saveBlockedTests()
      if lock.withLock({ stopped }) { batch.stop() }
      // Nobody running now, so nothing can end while the plan is asked: no answer means none will come.
      let idle = lock.withLock { running.isEmpty }
      var startedAny = false
      while let position = batch.startNext() {
        startedAny = true
        launch(Int(position), entry: entries[Int(position)], of: batch, sideBySide: sideBySide)
      }
      if batch.isOver() || (idle && !startedAny) { break }
      ended.wait()
    }
    // A test that ended while the loop was deciding to stop may have blocked others just now.
    saveBlockedTests()
    let results = lock.withLock { zip(entries, outcomes).map { TestResult(file: $0.file, outcome: $1 ?? .notRun) } }
    printSummary(results, duration: Date().timeIntervalSince(started))
    if request.notify, !lock.withLock({ stopped }) { notify(about: results.compactMap(\.dir)) }
    return results
  }

  /// Starts the test at `position` on a thread of its own and wakes `runAll` when it is over. With
  /// tests side by side, its lines are kept until it ends and then printed together.
  private func launch(_ position: Int, entry: BatchEntry, of batch: TestBatch, sideBySide: Bool) {
    let run = TestRun()
    let isStopped = lock.withLock {
      running[position] = run
      return stopped
    }
    if isStopped { run.cancel() }
    // A thread of its own: a test blocks for its whole run, and on the shared queue it would wait
    // for a free thread behind whatever else blocks there.
    Thread { [self] in
      let transcript = Transcript()
      let console = sideBySide ? Console(write: { transcript.append($0) }, color: progress.color) : progress
      let outcome = self.run((project as NSString).appendingPathComponent(entry.file), entry: entry, with: run, console: console)
      if sideBySide { emit(transcript.text) }
      lock.withLock { outcomes[position] = outcome }
      _ = batch.finish(position: UInt32(position), outcome: Self.testOutcome(outcome))
      lock.withLock { running[position] = nil }
      ended.signal()
    }.start()
  }

  /// What the tests that depend on this one go by.
  private static func testOutcome(_ outcome: TestResult.Outcome) -> TestOutcome {
    guard case .finished(_, _, let report) = outcome else { return .notRun }
    switch report.verdict {
    case .passed: return .passed
    case .cancelled: return .cancelled
    case .blocked, .unknown: return .notRun
    // A failure in quarantine still means the test didn't do what the ones after it need.
    case .failed: return .failed
    }
  }

  /// Saves the run of a test that didn't start because a test it depends on didn't pass.
  private func saveBlocked(_ entry: BatchEntry, _ blocker: Blocker) -> TestResult.Outcome {
    var text = "\n" + progress.paint(entry.name, .bold) + progress.paint("  \(entry.file)", .dim) + "\n"
    text += "  " + progress.paint(describeBlocker(blocker: blocker), .yellow) + "\n"
    let outcome = saveBlockedRun(
      project: project, test: entry.file, blocker: blocker,
      options: RunOptions(
        startAt: nil, retries: nil, environment: request.environment, vision: nil, localOnly: localOnly,
        schedule: request.schedule, video: .off, addedAsDependency: entry.added, language: RunLanguage.app))
    defer { emit(text) }
    switch outcome {
    case .finished(let dir, let html, let report):
      text += "  " + progress.paint(words("cli.run.report", html ?? dir), .dim) + "\n"
      return .finished(dir: dir, html: html, report: report)
    case .failed(let reason):
      text += "  " + progress.paint("✗ " + words("cli.run.notSaved", reason), .red) + "\n"
      return .error(reason)
    }
  }

  private func run(_ test: String, entry: BatchEntry, with run: TestRun, console: Console) -> TestResult.Outcome {
    let flow = (try? String(contentsOfFile: test, encoding: .utf8)).flatMap { try? loadFlow(json: $0) }
    // Without branches and loops every block runs once, so the count tells how far the test is.
    let linear = flow?.blocks.allSatisfy {
      switch $0.kind {
      case .if, .repeat, .useData, .runTest: false
      default: true
      }
    }
    let listener = Progress(
      console: console, blockCount: linear == true && request.startAt == nil ? flow?.blocks.count : nil
    ) { [input] in
      guard let input else { return run.resume() }
      input.resumeWhenIdle(run)
    }
    console.line()
    input?.ignoreInput(for: Self.handsOffTime)
    run.start(
      project: project, test: test, options: RunOptions(
        startAt: request.startAt, retries: request.retries, environment: request.environment, dataRows: request.rows,
        vision: vision,
        localOnly: localOnly, schedule: request.schedule, fallback: fallback, providers: providers, usage: usage,
        video: request.video ?? .off, keepVideosDays: keepVideosDays, addedAsDependency: entry.added,
        language: RunLanguage.app),
      hands: makeHands(run), secrets: StoredSecrets(), listener: listener)
    listener.done.wait()
    switch listener.outcome {
    case .finished(let dir, let html, let report)?:
      printVerdict(report, dir: dir, html: html ?? dir, to: console)
      return .finished(dir: dir, html: html, report: report)
    case .failed(let reason)?:
      console.line("  " + console.paint("✗ " + words("cli.run.notSaved", reason), .red))
      return .error(reason)
    case nil:
      return .error(words("cli.run.noResult"))
    }
  }

  /// Sends the message the project's notification settings ask for about the runs in `dirs`, one
  /// message about a single run and one summary about several, and says how it went.
  private func notify(about dirs: [String]) {
    guard !dirs.isEmpty else { return }
    let lines: [String]
    do {
      lines = try notifyRuns(project: project, dirs: dirs).map { sent in
        let channel = notifyChannelName(channel: sent.channel)
        guard let failure = sent.failure else { return "  " + words("cli.run.messageSent", channel) }
        return "  " + progress.paint("✗ " + words("cli.run.messageFailed", channel, notifyErrorMessage(error: failure)), .red)
      }
    } catch let error as NotifyError {
      lines = ["  " + progress.paint("✗ " + words("cli.run.noMessages", notifyErrorMessage(error: error)), .red)]
    } catch {
      lines = ["  " + progress.paint("✗ " + words("cli.run.noMessages", "\(error)"), .red)]
    }
    if !lines.isEmpty { progress.line() }
    lines.forEach { progress.line($0) }
  }

  /// Time to take the hand off the keyboard after pressing Return.
  private static let handsOffTime: TimeInterval = 1.5

  private func printVerdict(_ report: RunReport, dir: String, html: String, to console: Console) {
    let took = duration(report.durationMs)
    let attempts = report.attempts.count
    let line =
      switch report.verdict {
      case .passed where report.nothingChecked:
        console.paint(nothingChecked(), .yellow) + "  " + console.paint(took, .dim)
      case .passed:
        console.paint(report.flaky ? words("cli.run.passedFlaky", attempts, took) : words("cli.run.passed", took), .green)
      case .failed where report.quarantined: console.paint(words("cli.run.failedQuarantined", took), .yellow)
      case .failed: console.paint(words("cli.run.failed", took), .red)
      case .cancelled: console.paint(words("cli.run.stopped"), .yellow)
      case .blocked: console.paint(report.blocked.map { describeBlocker(blocker: $0) } ?? words("report.status.blocked"), .yellow)
      case .unknown: console.paint(words("report.status.unknown"), .yellow)
      }
    console.line("  " + line)
    if !report.rows.isEmpty {
      let passed = report.rows.count { $0.verdict == .passed }
      console.line("  " + words("cli.run.rowsPassed", report.rows.count, passed))
      for row in report.rows where row.verdict != .passed {
        let stopped = row.verdict == .cancelled
        console.line(
          "    " + console.paint(stopped ? words("cli.run.rowStopped", row.row.title) : words("cli.run.rowFailed", row.row.title), stopped ? .yellow : .red))
      }
    }
    if report.verdict == .failed, let verdict = retryVerdict(report: report) {
      console.line("  " + describeRetryVerdict(verdict: verdict))
    }
    if report.quarantined {
      let counts = request.includeQuarantinedFailures
      let text =
        switch report.quarantineReason {
        case let reason?: counts ? words("cli.run.quarantineCountsBecause", reason) : words("cli.run.quarantineHoldsBecause", reason)
        case nil: counts ? words("cli.run.quarantineCounts") : words("cli.run.quarantineHolds")
        }
      console.line("  " + text)
    }
    if !report.baselinesCreated.isEmpty {
      console.line("  " + words("cli.run.newReferences", report.baselinesCreated.joined(separator: ", ")))
    }
    console.line("  " + console.paint(words("cli.run.report", html), .dim))
    if let video = report.video {
      console.line("  " + console.paint(words("cli.run.video", URL(filePath: dir).appending(path: video).path), .dim))
    }
  }

  private func printSummary(_ results: [TestResult], duration seconds: TimeInterval) {
    let count = { (verdict: String) in results.count { $0.verdict == verdict } }
    var parts: [String] = []
    if count("passed") > 0 { parts.append(progress.paint(words("cli.summary.passed", count("passed")), .green)) }
    if count("nothingChecked") > 0 { parts.append(progress.paint(words("cli.summary.nothingChecked", count("nothingChecked")), .yellow)) }
    if count("failed") > 0 { parts.append(progress.paint(words("cli.summary.failed", count("failed")), .red)) }
    if count("quarantined") > 0 {
      parts.append(
        progress.paint(words("cli.summary.quarantined", count("quarantined")), request.includeQuarantinedFailures ? .red : .yellow))
    }
    if count("blocked") > 0 { parts.append(progress.paint(words("cli.summary.blocked", count("blocked")), .yellow)) }
    if count("cancelled") > 0 { parts.append(progress.paint(words("cli.summary.stopped", count("cancelled")), .yellow)) }
    if count("notRun") > 0 { parts.append(progress.paint(words("cli.summary.notRun", count("notRun")), .yellow)) }
    if count("error") > 0 { parts.append(progress.paint(words("cli.summary.notSaved", count("error")), .red)) }
    progress.line()
    progress.line(
      progress.paint(words("cli.summary.total", results.count, duration(UInt64(seconds * 1000))), .bold) + " "
        + parts.joined(separator: ", "))
  }
}

/// What a test printed, held until it ends so the lines of tests side by side don't mix.
private final class Transcript: @unchecked Sendable {
  private let lock = NSLock()
  private var parts: [String] = []

  func append(_ part: String) { lock.withLock { parts.append(part) } }
  var text: String { lock.withLock { parts.joined() } }
}

/// Prints a test's progress as it runs, from the run's thread.
private final class Progress: RunListener, @unchecked Sendable {
  let done = DispatchSemaphore(value: 0)
  private(set) var outcome: RunOutcome?
  private let console: Console
  private let blockCount: Int?
  private let paused: () -> Void
  private var file = ""
  private var maxAttempts: UInt32 = 1
  /// A step's line is on screen without its result yet.
  private var lineOpen = false
  private var failureShown = false

  init(console: Console, blockCount: Int?, paused: @escaping () -> Void) {
    self.console = console
    self.blockCount = blockCount
    self.paused = paused
  }

  func event(event: RunEvent) {
    switch event {
    case .runStarted(let test, let file, _, let maxAttempts):
      self.file = file
      self.maxAttempts = maxAttempts
      console.line(console.paint(test, .bold) + console.paint("  \(file)", .dim))
    case .rowStarted(let row, let rows, let label):
      closeLine()
      let title = DataRow(number: row, label: label).title
      console.line("  " + console.paint(words("cli.run.rowProgress", title, row, rows), .bold))
    case .attemptStarted(let attempt):
      failureShown = false
      if attempt > 1 { console.line("  " + words("cli.run.attempt", attempt, maxAttempts)) }
    case .stepStarted(_, let seq, let test, let block, let type, _, _):
      guard console.live else { return }
      console.write("  " + console.paint("…", .dim) + " " + label(seq, block, type, test))
      lineOpen = true
    case .stepFinished(_, let step):
      closeLine()
      let mark =
        switch step.status {
        case .passed: console.paint("✓", .green)
        case .baselineCreated: console.paint("✓", .green)
        case .failed: console.paint("✗", .red)
        case .cancelled: console.paint("–", .yellow)
        case .unknown: console.paint("?", .yellow)
        }
      var text = "  \(mark) \(label(step.seq, step.block, step.blockType, step.test))  "
      text += console.paint(duration(step.durationMs), .dim)
      if step.status == .baselineCreated, let reference = step.reference {
        text += "  " + words("cli.run.newReference", reference)
      }
      if let branch = branchText(step: step) {
        text += "  " + branch
      }
      console.line(text)
      for output in step.outputs {
        console.line("    " + console.paint("\(output.name) = \(output.value)", .dim))
      }
      if let failure = step.failure {
        console.line("    " + console.paint(describeFailure(failure: failure), .red))
        failureShown = true
      }
    case .paused(_, let reason, _):
      closeLine()
      guard reason == .userInput else { return }
      console.line("  " + console.paint(words("cli.run.paused", Int(InputWatcher.idleTime)), .yellow))
      paused()
    case .resumed:
      console.line("  " + words("cli.run.resumed"))
    case .waitingForScreen(let holder):
      closeLine()
      var text = words("cli.run.screenBusy")
      if let holder {
        let since = Date(timeIntervalSince1970: TimeInterval(holder.since))
          .formatted(Date.FormatStyle(date: .omitted, time: .shortened).locale(Locale(identifier: currentLanguage())))
        text = words("cli.run.screenBusyWith", holder.process, holder.test, since)
      }
      console.line("  " + console.paint(text, .yellow))
    case .attemptFinished(_, _, let failure):
      closeLine()
      if let failure, !failureShown {
        console.line("  " + console.paint("✗ " + describeFailure(failure: failure), .red))
      }
    case .runFinished:
      break
    }
  }

  func finished(outcome: RunOutcome) {
    closeLine()
    self.outcome = outcome
    done.signal()
  }

  private func label(_ seq: UInt32, _ block: String, _ type: String, _ test: String) -> String {
    let number = blockCount.map { "\(seq)/\($0)" } ?? "\(seq)"
    var text = "\(number) \(block) " + console.paint("· \(Self.name(of: type))", .dim)
    if test != file { text += console.paint("  " + words("cli.run.inTest", test), .dim) }
    return text
  }

  /// Ends the line of a step that is still going, so the next text starts on its own line.
  private func closeLine() {
    guard lineOpen else { return }
    console.write("\r\u{1B}[2K")
    lineOpen = false
  }

  /// The block type in words, as the steps of a run list it.
  private static func name(of type: String) -> String {
    switch type {
    case "openApp": words("cli.block.openApp")
    case "openFile": words("cli.block.openFile")
    case "click": words("cli.block.click")
    case "hover": words("cli.block.hover")
    case "drag": words("cli.block.drag")
    case "scroll": words("cli.block.scroll")
    case "type": words("cli.block.type")
    case "pressKeys": words("cli.block.pressKeys")
    case "waitFor": words("cli.block.waitFor")
    case "pause": words("cli.block.pause")
    case "check": words("cli.block.check")
    case "if": words("cli.block.if")
    case "repeat": words("cli.block.repeat")
    case "useData": words("cli.block.useData")
    case "runTest": words("cli.block.runTest")
    case "script": words("cli.block.script")
    case "command": words("cli.block.command")
    case "http": words("cli.block.http")
    case "checkFile": words("cli.block.checkFile")
    case "database": words("cli.block.database")
    case "clipboard": words("cli.block.clipboard")
    case "dateTime": words("cli.block.dateTime")
    default: type
    }
  }
}

/// `1.4 s`, `12 s`, `2 min 5 s`, in the language of the command line.
func duration(_ milliseconds: UInt64) -> String {
  switch milliseconds {
  case ..<10_000:
    let seconds = (Double(milliseconds) / 1000).formatted(.number.precision(.fractionLength(1)).locale(Locale(identifier: currentLanguage())))
    return words("report.duration.s", seconds)
  case ..<60_000: return words("report.duration.s", String(milliseconds / 1000))
  default: return words("notify.duration.minutes", milliseconds / 60_000, milliseconds / 1000 % 60)
  }
}

/// Ctrl-C (SIGINT) and SIGTERM, which CI sends to cancel a job, stop the run instead of killing
/// jevision, so the run still ends with a report. A second one quits at once.
enum Interrupts {
  static func install(_ handler: @escaping @Sendable () -> Void) -> [DispatchSourceSignal] {
    [SIGINT, SIGTERM].map { number in
      signal(number, SIG_IGN)
      let source = DispatchSource.makeSignalSource(signal: number, queue: .global())
      source.setEventHandler {
        signal(SIGINT, SIG_DFL)
        signal(SIGTERM, SIG_DFL)
        handler()
      }
      source.resume()
      return source
    }
  }
}

extension DataRow {
  /// "Row 2: anna", or "Row 2" when the row has no label.
  var title: String { label.isEmpty ? words("report.row.titleBare", number) : words("report.row.title", number, label) }
}

extension RowResult {
  var json: JSON {
    .object([
      ("row", .int(Int64(row.number))),
      ("label", .string(row.label)),
      ("verdict", .string(String(describing: verdict))),
      ("flaky", .bool(flaky)),
      ("attempts", .int(Int64(attempts))),
      ("failure", failure.map(\.json) ?? .null),
    ])
  }
}

extension Failure {
  var json: JSON {
    .object([
      ("code", .string(code)), ("message", .string(describeFailure(failure: self))),
      ("params", .object(params.sorted { $0.key < $1.key }.map { ($0.key, .string($0.value)) })),
    ])
  }
}

extension Blocker {
  var json: JSON {
    let reason: String
    var cycle = JSON.null
    switch self.reason {
    case .failed: reason = "failed"
    case .notRun: reason = "notRun"
    case .missing: reason = "missing"
    case .cycle(let tests):
      reason = "cycle"
      cycle = .array(tests.map { .string($0) })
    }
    return .object([
      ("reason", .string(reason)), ("dependency", .string(dependency)), ("cause", .string(cause)), ("cycle", cycle),
      ("message", .string(describeBlocker(blocker: self))),
    ])
  }
}
