import Foundation
import JEVisionCore
import JEVisionHands

/// `jevision run`: runs tests one after another on this Mac, shows how it goes and saves reports.
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
      return .failure
    }
  }

  /// Runs the tests one after another; the error is what to tell when they can't start.
  func runTests() throws(UsageError) -> [TestResult] {
    let plan: (project: String, tests: [String], environment: String?)
    do {
      plan = try Self.plan(request)
    } catch {
      throw UsageError(message: "jevision: \(error.message)")
    }
    let ai = AISettings.app
    let vision = ai.settings(provider: request.provider, model: request.model)
    if request.model != nil, vision == nil {
      throw UsageError(message: "jevision: --model needs an AI provider: choose one with --provider or in the app (Settings → AI)")
    }
    if let problem = Readiness.problem() {
      throw UsageError(message: problem)
    }
    let input = InputWatcher()
    let videos = VideoSettings.app
    var request = request
    request.environment = plan.environment
    request.video = request.video ?? videos.mode
    let session = TestSession(
      project: plan.project, tests: plan.tests, request: request, progress: progress, input: input,
      vision: vision, fallback: vision.flatMap { ai.fallback(besides: $0.provider) }, usage: ai.usage,
      localOnly: ai.localOnly, keepVideosDays: videos.keepDays
    ) { run in
      MacHands(input: input, isCancelled: { run.isCancelled() })
    }
    // Esc Esc stops the run right on the watcher's thread.
    guard input.start(onEmergencyStop: { [session] in session.stop() }) else {
      throw UsageError(message: "jevision: macOS didn’t let jevision watch the keyboard, so Esc Esc couldn’t stop a run.")
    }
    defer { input.stop() }
    let interrupts = Interrupts.install { [session, progress] in
      session.stop()
      progress.line(progress.paint("Press Ctrl-C again to quit at once.", .dim))
    }
    return withExtendedLifetime(interrupts) { session.runAll() }
  }

  // MARK: Planning

  /// The project folder and the test files to run, as real absolute paths, and the environment
  /// to run in: the one asked for, otherwise the default of the project's environments file.
  static func plan(_ request: RunRequest) throws(UsageError) -> (project: String, tests: [String], environment: String?) {
    var targets: [String] = []
    for path in request.paths {
      // The app lists tests without the suffix, so `notes/add-milk` runs notes/add-milk.flow.json.
      guard let real = realPath(path) ?? realPath(path + ".flow.json") else {
        throw UsageError(message: "there is no file or folder “\(path)”")
      }
      targets.append(real)
    }
    let (tests, _) = collectTests(targets)
    guard !tests.isEmpty else {
      throw UsageError(message: "there are no tests (*.flow.json files) in \(request.paths.joined(separator: ", "))")
    }
    let project: String
    if let given = request.project {
      guard let real = realPath(given), isFolder(real) else { throw UsageError(message: "there is no folder “\(given)”") }
      project = real
    } else {
      project = defaultProject(for: targets[0], tests: tests)
    }
    if let outside = tests.first(where: { !isInside($0, project) }) {
      throw UsageError(
        message: "“\(outside)” is outside the project folder \(project). Choose a folder that holds all the tests with --project.")
    }
    guard let environment = request.environment else {
      return (project, tests, try readEnvironments(in: project) { try defaultEnvironment(project: project) })
    }
    try checkEnvironment(environment, in: project)
    return (project, tests, environment)
  }

  /// Fails unless the project has the environment `name`, saying which ones it has.
  private static func checkEnvironment(_ name: String, in project: String) throws(UsageError) {
    let file = environmentsFileName()
    let all = try readEnvironments(in: project) { try projectEnvironments(project: project) }
    guard !all.contains(where: { $0.name == name }) else { return }
    if all.isEmpty {
      throw UsageError(message: "there is no environment “\(name)”: the project has no \(file) in \(project)")
    }
    throw UsageError(
      message: "there is no environment “\(name)” in \(file); use one of: \(all.map(\.name).joined(separator: ", "))")
  }

  /// What `read` gets from the environments file of `project`, or why the file can't be read.
  private static func readEnvironments<T>(in project: String, _ read: () throws -> T) throws(UsageError) -> T {
    let file = environmentsFileName()
    do {
      return try read()
    } catch EnvironmentsError.Unreadable(_, let reason) {
      throw UsageError(message: "\(file) in \(project) can’t be read: \(reason)")
    } catch {
      throw UsageError(message: "\(file) in \(project) can’t be read: \(error.localizedDescription)")
    }
  }

  /// The nearest folder above `target` that has a `.jevision` folder, as the app makes one in
  /// the project it opens, or the environments file; otherwise the current folder if it holds all the tests; otherwise
  /// the folder of `target`.
  private static func defaultProject(for target: String, tests: [String]) -> String {
    let start = isFolder(target) ? target : (target as NSString).deletingLastPathComponent
    var folder = start
    while true {
      if isProjectFolder(folder) { return folder }
      guard folder != "/" else { break }
      folder = (folder as NSString).deletingLastPathComponent
    }
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

  private static func realPath(_ path: String) -> String? {
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
    if let junit = request.junit {
      do {
        try exportJunit(dirs: runs, to: junit, includeQuarantinedFailures: request.includeQuarantinedFailures)
        progress.line(progress.paint("JUnit report: \(junit)", .dim))
      } catch {
        Console.standardError.line("\(junit): cannot write: \(error)")
        status = .failure
      }
    }
    if let folder = request.html {
      do {
        try FileManager.default.createDirectory(atPath: folder, withIntermediateDirectories: true)
        for dir in runs {
          try exportHtml(dir: dir, to: (folder as NSString).appendingPathComponent((dir as NSString).lastPathComponent + ".html"))
        }
        if !runs.isEmpty { progress.line(progress.paint("HTML reports: \(folder)", .dim)) }
      } catch {
        Console.standardError.line("\(folder): cannot write: \(error)")
        status = .failure
      }
    }
    if request.json { output.line(Self.json(results).pretty) }
    return status
  }

  static func json(_ results: [TestResult]) -> JSON {
    let count = { (verdict: String) in JSON.int(Int64(results.count { $0.verdict == verdict })) }
    return .object([
      ("passed", count("passed")),
      ("nothingChecked", count("nothingChecked")),
      ("failed", count("failed")),
      ("quarantined", count("quarantined")),
      ("cancelled", count("cancelled")),
      ("notRun", count("notRun")),
      ("errors", count("error")),
      ("tests", .array(results.map(\.json))),
    ])
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
      }
    case .error: "error"
    case .notRun: "notRun"
    }
  }

  /// A failure in quarantine doesn't count unless asked: see `RunRequest.includeQuarantinedFailures`.
  var status: ExitStatus {
    switch verdict {
    case "passed", "quarantined", "nothingChecked": .ok
    case "error": .failure
    default: .problems
    }
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
        ("attempts", .int(Int64(report.attempts.count))),
        ("rows", .array(report.rows.map(\.json))),
        ("durationMs", .int(Int64(report.durationMs))),
        ("newReferences", .array(report.baselinesCreated.map { .string($0) })),
        ("failure", failure.map { .object([("code", .string($0.code)), ("message", .string(describeFailure(failure: $0)))]) } ?? .null),
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

/// Runs the tests one at a time; `stop` cancels the one running and skips the rest.
final class TestSession: @unchecked Sendable {
  private let project: String
  private let tests: [String]
  private let request: RunRequest
  private let progress: Console
  private let makeHands: (TestRun) -> any Hands
  private let vision: VisionSettings?
  private let fallback: VisionSettings?
  private let usage: AiUsageSettings?
  private let localOnly: Bool
  private let keepVideosDays: UInt32?
  /// The person's mouse and keyboard, watched while a test runs.
  private let input: InputWatcher?
  private let lock = NSLock()
  private var current: TestRun?
  private var stopped = false

  init(
    project: String, tests: [String], request: RunRequest, progress: Console, input: InputWatcher?,
    vision: VisionSettings? = nil, fallback: VisionSettings? = nil, usage: AiUsageSettings? = nil,
    localOnly: Bool = false, keepVideosDays: UInt32? = nil, makeHands: @escaping (TestRun) -> any Hands
  ) {
    self.vision = vision
    self.fallback = fallback
    self.usage = usage
    self.localOnly = localOnly
    self.keepVideosDays = keepVideosDays
    self.project = project
    self.tests = tests
    self.request = request
    self.progress = progress
    self.input = input
    self.makeHands = makeHands
  }

  /// Stops the test that is running, which is then reported as cancelled, and the ones after it.
  func stop() {
    let (run, wasStopped) = lock.withLock {
      defer { stopped = true }
      return (current, stopped)
    }
    run?.cancel()
    guard !wasStopped else { return }
    progress.line()
    progress.line(progress.paint("Stopping…", .yellow))
  }

  func runAll() -> [TestResult] {
    let started = Date()
    let count = tests.count == 1 ? "1 test" : "\(tests.count) tests"
    progress.line("Running \(count) in \(project). Press Esc twice or Ctrl-C to stop.")
    var results: [TestResult] = []
    for test in tests {
      let file = String(test.dropFirst(project == "/" ? 1 : project.count + 1))
      let run = TestRun()
      let isStopped = lock.withLock {
        current = run
        return stopped
      }
      guard !isStopped else {
        results.append(TestResult(file: file, outcome: .notRun))
        continue
      }
      results.append(TestResult(file: file, outcome: self.run(test, with: run)))
    }
    lock.withLock { current = nil }
    printSummary(results, duration: Date().timeIntervalSince(started))
    return results
  }

  private func run(_ test: String, with run: TestRun) -> TestResult.Outcome {
    let flow = (try? String(contentsOfFile: test, encoding: .utf8)).flatMap { try? loadFlow(json: $0) }
    // Without branches and loops every block runs once, so the count tells how far the test is.
    let linear = flow?.blocks.allSatisfy {
      switch $0.kind {
      case .if, .repeat, .useData, .runTest: false
      default: true
      }
    }
    let listener = Progress(
      console: progress, blockCount: linear == true && request.startAt == nil ? flow?.blocks.count : nil
    ) { [input] in
      guard let input else { return run.resume() }
      Self.resumeWhenIdle(run, input: input)
    }
    progress.line()
    input?.ignoreInput(for: Self.handsOffTime)
    run.start(
      project: project, test: test, options: RunOptions(
        startAt: request.startAt, retries: request.retries, environment: request.environment, dataRows: request.rows,
        vision: vision,
        localOnly: localOnly, schedule: request.schedule, fallback: fallback, usage: usage,
        video: request.video ?? .off, keepVideosDays: keepVideosDays),
      hands: makeHands(run), secrets: StoredSecrets(), listener: listener)
    listener.done.wait()
    switch listener.outcome {
    case .finished(let dir, let html, let report)?:
      printVerdict(report, dir: dir, html: html ?? dir)
      return .finished(dir: dir, html: html, report: report)
    case .failed(let reason)?:
      progress.line("  " + progress.paint("✗ The run couldn’t be saved: \(reason)", .red))
      return .error(reason)
    case nil:
      return .error("The run ended without a result.")
    }
  }

  /// Time to take the hand off the keyboard after pressing Return.
  private static let handsOffTime: TimeInterval = 1.5
  /// How long the mouse and keyboard must rest before a paused test goes on.
  static let idleTime: TimeInterval = 3

  /// A person took the mouse or keyboard and the test paused: it goes on once they let go for
  /// `idleTime`, unless the run is stopped first.
  static func resumeWhenIdle(_ run: TestRun, input: InputWatcher) {
    Thread.detachNewThread {
      var quietSince = Date()
      while !run.isCancelled() {
        Thread.sleep(forTimeInterval: 0.25)
        if input.takeUserInput() {
          quietSince = Date()
        } else if Date().timeIntervalSince(quietSince) >= Self.idleTime {
          run.resume()
          return
        }
      }
    }
  }

  private func printVerdict(_ report: RunReport, dir: String, html: String) {
    let took = duration(report.durationMs)
    let attempts = report.attempts.count
    let line =
      switch report.verdict {
      case .passed where report.nothingChecked:
        progress.paint("Finished in \(took), but nothing was checked: the test has no check or wait block that looks at the screen", .yellow)
      case .passed:
        progress.paint(
          report.flaky ? "Passed on attempt \(attempts) in \(took). It failed before, so it may be flaky." : "Passed in \(took)",
          .green)
      case .failed where report.quarantined: progress.paint("Failed in \(took), in quarantine", .yellow)
      case .failed: progress.paint("Failed in \(took)", .red)
      case .cancelled: progress.paint("Stopped", .yellow)
      }
    progress.line("  " + line)
    if !report.rows.isEmpty {
      let passed = report.rows.count { $0.verdict == .passed }
      let rows = report.rows.count == 1 ? "1 row" : "\(report.rows.count) rows"
      progress.line("  \(passed) of \(rows) passed")
      for row in report.rows where row.verdict != .passed {
        let color: Console.Style = row.verdict == .cancelled ? .yellow : .red
        let verdict = row.verdict == .cancelled ? "stopped" : "failed"
        progress.line("    " + progress.paint("\(row.row.title) \(verdict)", color))
      }
    }
    if report.verdict == .failed, let verdict = retryVerdict(report: report) {
      progress.line("  " + describeRetryVerdict(verdict: verdict))
    }
    if report.quarantined {
      let reason = report.quarantineReason.map { " (\($0))" } ?? ""
      progress.line(
        "  In quarantine\(reason): "
          + (request.includeQuarantinedFailures ? "its failures count." : "its failures don't fail the run."))
    }
    if !report.baselinesCreated.isEmpty {
      progress.line("  New reference images: \(report.baselinesCreated.joined(separator: ", ")). Look at them and commit them.")
    }
    progress.line("  " + progress.paint("Report: \(html)", .dim))
    if let video = report.video {
      progress.line("  " + progress.paint("Video: \(URL(filePath: dir).appending(path: video).path)", .dim))
    }
  }

  private func printSummary(_ results: [TestResult], duration seconds: TimeInterval) {
    let count = { (verdict: String) in results.count { $0.verdict == verdict } }
    var parts: [String] = []
    if count("passed") > 0 { parts.append(progress.paint("\(count("passed")) passed", .green)) }
    if count("nothingChecked") > 0 { parts.append(progress.paint("\(count("nothingChecked")) checked nothing", .yellow)) }
    if count("failed") > 0 { parts.append(progress.paint("\(count("failed")) failed", .red)) }
    if count("quarantined") > 0 {
      parts.append(progress.paint("\(count("quarantined")) failed in quarantine", request.includeQuarantinedFailures ? .red : .yellow))
    }
    if count("cancelled") > 0 { parts.append(progress.paint("\(count("cancelled")) stopped", .yellow)) }
    if count("notRun") > 0 { parts.append(progress.paint("\(count("notRun")) not run", .yellow)) }
    if count("error") > 0 { parts.append(progress.paint("\(count("error")) couldn’t be saved", .red)) }
    let total = results.count == 1 ? "1 test" : "\(results.count) tests"
    progress.line()
    progress.line(progress.paint("\(total) in \(duration(UInt64(seconds * 1000))): ", .bold) + parts.joined(separator: ", "))
  }
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
      console.line("  " + console.paint("\(title) (\(row) of \(rows))", .bold))
    case .attemptStarted(let attempt):
      failureShown = false
      if attempt > 1 { console.line("  Attempt \(attempt) of \(maxAttempts)") }
    case .stepStarted(_, let seq, let test, let block, let type, _):
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
        }
      var text = "  \(mark) \(label(step.seq, step.block, step.blockType, step.test))  "
      text += console.paint(duration(step.durationMs), .dim)
      if step.status == .baselineCreated, let reference = step.reference {
        text += "  new reference \(reference)"
      }
      if let branch = branchText(step: step) {
        text += "  " + branch
      }
      console.line(text)
      if let failure = step.failure {
        console.line("    " + console.paint(describeFailure(failure: failure), .red))
        failureShown = true
      }
    case .paused(_, let reason, _):
      closeLine()
      guard reason == .userInput else { return }
      console.line(
        console.paint(
          "  Paused: someone used the mouse or keyboard. The test goes on when they rest for \(Int(TestSession.idleTime)) seconds.",
          .yellow))
      paused()
    case .resumed:
      console.line("  Going on")
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
    var text = "\(number) \(block) " + console.paint("· \(Self.names[type] ?? type)", .dim)
    if test != file { text += console.paint("  in \(test)", .dim) }
    return text
  }

  /// Ends the line of a step that is still going, so the next text starts on its own line.
  private func closeLine() {
    guard lineOpen else { return }
    console.write("\r\u{1B}[2K")
    lineOpen = false
  }

  private static let names = [
    "openApp": "open app", "openFile": "open file", "click": "click", "hover": "hover", "drag": "drag",
    "scroll": "scroll", "type": "type text",
    "pressKeys": "press keys", "waitFor": "wait", "check": "check", "if": "if", "repeat": "repeat",
    "useData": "use data", "runTest": "run test",
  ]
}

/// `1.4 s`, `12 s`, `2 min 5 s`.
func duration(_ milliseconds: UInt64) -> String {
  switch milliseconds {
  case ..<10_000: String(format: "%.1f s", Double(milliseconds) / 1000)
  case ..<60_000: "\(milliseconds / 1000) s"
  default: "\(milliseconds / 60_000) min \(milliseconds / 1000 % 60) s"
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
  var title: String { label.isEmpty ? "Row \(number)" : "Row \(number): \(label)" }
}

extension RowResult {
  var json: JSON {
    .object([
      ("row", .int(Int64(row.number))),
      ("label", .string(row.label)),
      ("verdict", .string(String(describing: verdict))),
      ("flaky", .bool(flaky)),
      ("attempts", .int(Int64(attempts))),
      ("failure", failure.map { .string(describeFailure(failure: $0)) } ?? .null),
    ])
  }
}
