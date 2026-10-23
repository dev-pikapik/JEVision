import Foundation
import JEVisionCore
import Testing

@testable import JEVision

/// How long the window takes on a made-up project of a thousand tests and thousands of saved
/// runs, along the paths it takes. Each measurement prints `BENCH\t<name>\t<wall ms>\t<cpu ms>`,
/// the least of three; `scripts/bench-scale.sh --app` collects them. Heavy, so it runs only with
/// `JEVISION_SCALE=1`.
@MainActor
@Suite(.enabled(if: ProcessInfo.processInfo.environment["JEVISION_SCALE"] == "1"), .serialized)
struct ScaleBenchmarkTests {
  static func project() throws -> (root: URL, summary: SyntheticSummary) { try ScaleBench.project() }

  private static func milliseconds(_ duration: Duration) -> Double { ScaleBench.milliseconds(duration) }

  /// Prints a measurement made by hand, with the CPU time when there is one.
  private static func report(_ name: String, _ wall: Double, cpu: Double? = nil) {
    ScaleBench.report(name, wall: wall, cpu: cpu)
  }

  /// Times `work` three times, prints the fastest and returns it in milliseconds.
  @discardableResult
  private func bench(_ name: String, _ work: () throws -> Void) rethrows -> Double {
    try ScaleBench.bench(name, work).wall
  }

  private func bench(_ name: String, _ work: () async throws -> Void) async rethrows {
    try await ScaleBench.bench(name, work)
  }

  /// Waits until `done`, looking every millisecond, for at most five minutes.
  private func wait(for what: String, until done: () -> Bool) async throws {
    let deadline = ContinuousClock.now + .seconds(300)
    while !done() {
      try #require(ContinuousClock.now < deadline, "Gave up waiting for \(what) after five minutes")
      try await Task.sleep(for: .milliseconds(1))
    }
  }

  private func opened() async throws -> ProjectStore {
    let (root, summary) = try Self.project()
    let store = ProjectStore()
    store.open(root)
    try await wait(for: "the project to open") { store.status == .ready && store.runs.count == Int(summary.reports) }
    return store
  }

  private var nowMs: Int64 { Int64(Date.now.timeIntervalSince1970 * 1000) }

  /// From opening to the tests and every run listed; `.tests` is until the tests show, `.main`
  /// the CPU time of the main thread over all of it, which the window can't draw during.
  @Test func openingTheProject() async throws {
    let (root, summary) = try Self.project()
    #expect(pthread_main_np() != 0)
    let clock = ContinuousClock()
    var best = (wall: Double.infinity, cpu: Double.infinity, main: Double.infinity, tests: Double.infinity)
    for _ in 0..<3 {
      let start = clock.now
      let cpu = ScaleBench.cpuMilliseconds()
      let main = ScaleBench.threadCPUMilliseconds()
      let store = ProjectStore()
      store.open(root)
      try await wait(for: "the tests to show") { store.status == .ready }
      let tests = Self.milliseconds(clock.now - start)
      try await wait(for: "the runs to show") { store.runs.count == Int(summary.reports) }
      best = (
        min(best.wall, Self.milliseconds(clock.now - start)), min(best.cpu, ScaleBench.cpuMilliseconds() - cpu),
        min(best.main, ScaleBench.threadCPUMilliseconds() - main), min(best.tests, tests)
      )
      #expect(store.items.flatMap(\.tests).count == summary.tests.count)
    }
    Self.report("app.project.open", best.wall, cpu: best.cpu)
    Self.report("app.project.open.tests", best.tests)
    Self.report("app.project.open.main", best.wall, cpu: best.main)
  }

  /// Looking through the project again after a change on disk: all of it, and with nothing changed
  /// since the scan before.
  @Test func scanningTheProject() throws {
    let root = try Self.project().root
    bench("app.project.scan.cold") { _ = try? ProjectScanner.scan(root) }
    let cache = ProjectScanner.Cache()
    _ = try ProjectScanner.scan(root, cache: cache)
    bench("app.project.scan.warm") { _ = try? ProjectScanner.scan(root, cache: cache) }
  }

  /// Fifty runs saved while the project is open, as during Run All: from the first written to
  /// all of them listed. The folder watcher gathers changes for up to 0.3 s, so that is part of it.
  @Test func rescanningWhileRunsAreSaved() async throws {
    let runs = try Self.project().root.appending(path: ".jevision/runs")
    let store = try await opened()
    let before = store.runs.count
    let source = try #require(store.runs.first).dir
    let copies = (0..<50).map { runs.appending(path: String(format: "29991231-0000%02d-scale-bench", $0)) }
    let clock = ContinuousClock()
    var best = (wall: Double.infinity, cpu: Double.infinity, main: Double.infinity)
    for _ in 0..<3 {
      let start = clock.now
      let cpu = ScaleBench.cpuMilliseconds()
      let main = ScaleBench.threadCPUMilliseconds()
      for copy in copies { try FileManager.default.copyItem(at: URL(filePath: source), to: copy) }
      try await wait(for: "the new runs to be listed") { store.runs.count == before + copies.count }
      best = (
        min(best.wall, Self.milliseconds(clock.now - start)), min(best.cpu, ScaleBench.cpuMilliseconds() - cpu),
        min(best.main, ScaleBench.threadCPUMilliseconds() - main)
      )
      for copy in copies { try FileManager.default.removeItem(at: copy) }
      try await wait(for: "the deleted runs to go") { store.runs.count == before }
    }
    Self.report("app.project.rescanDuringRun", best.wall, cpu: best.cpu)
    Self.report("app.project.rescanDuringRun.main", best.wall, cpu: best.main)
  }

  @Test func filteringTheSidebar() throws {
    let items = try ProjectScanner.scan(Self.project().root)
    let runs = try listRuns(project: Self.project().root.path)
    let flaky = Set(flakyTests(runs: runs.map(\.report), nowMs: nowMs).keys)
    let filters = [
      TestFilter(priority: .high), TestFilter(testType: .smoke, status: .ready), TestFilter(author: "Anna"),
      TestFilter(flaky: true),
    ]
    let total = bench("app.sidebar.filter.all") {
      for filter in filters {
        _ = items.keeping(Set(items.chosen(by: filter, flaky: flaky).map(\.id)))
      }
    }
    Self.report("app.sidebar.filter", total / Double(filters.count))
  }

  /// The sidebar's search, once per letter typed.
  @Test func searchingTheSidebar() throws {
    let items = try ProjectScanner.scan(Self.project().root)
    let queries = ["add", "coupon", "checkout coupon", "dark mode", "rename tag", "zzz"]
    let keystrokes = queries.map(\.count).reduce(0, +)
    let total = bench("app.search.rank.all_keystrokes") {
      for query in queries {
        for end in 1...query.count { _ = ProjectScanner.tests(in: items, matching: String(query.prefix(end))) }
      }
    }
    Self.report("app.search.rank", total / Double(keystrokes))
  }

  /// The board of all tests: worked out once per change, then asked what is in view on every
  /// step of a pan or zoom.
  @Test func showingTheBoardOfAllTests() throws {
    let text = try String(contentsOf: Self.project().root.appending(path: "boards/All Tests.canvas"), encoding: .utf8)
    let board = try loadBoard(text: text)
    bench("app.board.scene") { _ = BoardScene(board) }
    bench("app.board.save") { _ = saveBoard(board: board) }
    let scene = BoardScene(board)
    let bounds = try #require(scene.layout.bounds)
    // A window's worth at full size and at a quarter, panned in steps over the whole board.
    var views: [CGRect] = []
    for size in [CGSize(width: 1440, height: 900), CGSize(width: 5760, height: 3600)] {
      for step in 0..<100 {
        let x: Double = bounds.x + bounds.width * Double(step % 10) / 10
        let y: Double = bounds.y + bounds.height * Double(step / 10) / 10
        views.append(CGRect(origin: CGPoint(x: x, y: y), size: size))
      }
    }
    let total = bench("app.board.panzoom.all_views") {
      for area in views { _ = scene.visible(in: area) }
    }
    Self.report("app.board.panzoom", total / Double(views.count))
  }

  @Test func openingTheLargestTest() throws {
    let (root, summary) = try Self.project()
    #expect(summary.largestBlocks == 200)
    bench("app.flow.open.200") {
      let document = FlowDocument(url: root.appending(path: summary.largest), project: root)
      #expect(document.order.count >= 200)
    }
  }

  @Test func showingTheHistory() throws {
    let runs = try listRuns(project: Self.project().root.path)
    bench("app.runs.days") { _ = RunHistory.days(runs) }
    bench("app.runs.latestRuns") { _ = ProjectMap.latestRuns(runs) }
    let file = try #require(runs.first).report.file
    let ofOne = runs.map(\.report).filter { $0.file == file }
    bench("app.runs.testHealth") { _ = testHealth(runs: ofOne, nowMs: nowMs) }
  }

  /// The runs list as a name is typed into its search, once per letter.
  @Test func searchingTheHistory() async throws {
    let store = try await opened()
    let queries = ["checkout", "sign in", "zzz"]
    let keystrokes = queries.map(\.count).reduce(0, +)
    let total = bench("app.runs.list.all_keystrokes") {
      for query in queries {
        for end in 1...query.count { _ = store.runDays(matching: String(query.prefix(end)), from: nil) }
      }
    }
    Self.report("app.runs.list", total / Double(keystrokes))
  }

  /// What the open test shows of its last run, worked out on each redraw of the editor: for a test
  /// with no run made here, every run is looked through.
  @Test func findingTheLastRun() async throws {
    let store = try await opened()
    let (root, summary) = try Self.project()
    let document = FlowDocument(url: root.appending(path: summary.largest), project: root)
    let flow = try #require(document.flow)
    bench("app.runs.lastRun.none") { _ = LastRun(file: "never-ran.flow.json", flow: flow, runs: store.runs, pinned: nil) }
    bench("app.runs.lastRun") { _ = LastRun(file: summary.largest, flow: flow, runs: store.runs, pinned: nil) }
    bench("app.runs.lastRun.index") { _ = LastRun(file: "never-ran.flow.json", flow: flow, index: store.runIndex, pinned: nil) }
  }

  /// A card of the board of all tests moved twenty times, as dragging it and letting go does;
  /// per move, with the CPU time of the main thread.
  @Test func editingTheBoardOfAllTests() async throws {
    let store = try await opened()
    let boards = store.boards
    let start = try #require(boards.board)
    let moves = 20
    let clock = ContinuousClock()
    var best = (wall: Double.infinity, main: Double.infinity)
    for round in 0..<3 {
      let begin = clock.now
      let main = ScaleBench.threadCPUMilliseconds()
      for move in 1...moves {
        var board = try #require(boards.board)
        board.nodes[0].rect.x = start.nodes[0].rect.x + Double(round * moves + move)
        try boards.edit(board, action: "Move", undoManager: nil)
      }
      best = (min(best.wall, Self.milliseconds(clock.now - begin)), min(best.main, ScaleBench.threadCPUMilliseconds() - main))
    }
    try boards.edit(start, action: "Move", undoManager: nil)
    Self.report("app.board.edit", best.wall / Double(moves), cpu: best.main / Double(moves))
  }

  /// The report of the run with the most steps, opened as its window does.
  @Test func openingAReport() throws {
    let runs = try listRuns(project: Self.project().root.path)
    let steps = { (run: SavedRun) in run.report.attempts.map(\.steps.count).reduce(0, +) }
    let largest = try #require(runs.max { steps($0) < steps($1) })
    try bench("app.runs.loadReport") {
      let loaded = try loadRun(dir: largest.dir)
      _ = RunReportView.decidingIndex(loaded.report)
      _ = RunHistory.failedStep(loaded.report)
    }
  }
}
