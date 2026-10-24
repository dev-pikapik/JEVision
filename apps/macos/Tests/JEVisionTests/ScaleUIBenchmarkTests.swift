import AppKit
import Foundation
import JEVisionCore
import SwiftUI
import Testing

@testable import JEVision

/// What the window itself costs on the made-up project of a thousand tests: SwiftUI working out
/// and drawing the sidebar, the board, a test of 200 blocks and a report of 10,000 steps, in a
/// window kept off screen, without any input sent to the Mac. Each measurement prints
/// `BENCH\t<name>\t<wall ms>\t<cpu ms>`, the best of three, where the CPU time is the main
/// thread's alone: the work that keeps the window from answering. Work moved to other threads
/// doesn't count, as it doesn't stall the window. Heavy, so it runs only with `JEVISION_SCALE=1`.
@MainActor
@Suite(.enabled(if: ProcessInfo.processInfo.environment["JEVISION_SCALE"] == "1"), .serialized)
struct ScaleUIBenchmarkTests {
  /// A window off screen with a view in it, drawn when asked.
  @MainActor
  final class Offscreen {
    let window: NSWindow
    init(width: Double = 1200, height: Double = 760) {
      window = NSWindow(
        contentRect: NSRect(x: -10000, y: -10000, width: width, height: height), styleMask: [.titled, .resizable],
        backing: .buffered, defer: false)
      window.isReleasedWhenClosed = false
    }

    func show(_ view: some View) {
      window.contentView = NSHostingView(rootView: view)
      window.orderBack(nil)
    }

    /// Lets SwiftUI take in what changed, lays the window out and draws it.
    func draw() {
      RunLoop.main.run(until: Date())
      window.contentView?.layoutSubtreeIfNeeded()
      window.displayIfNeeded()
    }

    /// Draws until the main thread has had nothing to do for 0.2 s, or `done` holds and it has
    /// had nothing to do for that long since; at most `limit`.
    func settle(limit: Duration = .seconds(30), until done: () -> Bool = { true }) {
      let deadline = ContinuousClock.now + limit
      var quiet = 0
      while ContinuousClock.now < deadline {
        let before = Self.mainCPU()
        draw()
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        quiet = Self.mainCPU() - before < 1 ? quiet + 1 : 0
        if quiet >= 4, done() { return }
      }
    }

    /// Async views load on the main actor too; yield between draws so their tasks can run.
    func settleAsync(limit: Duration = .seconds(30), until done: () -> Bool) async {
      let deadline = ContinuousClock.now + limit
      var quiet = 0
      while ContinuousClock.now < deadline {
        let before = Self.mainCPU()
        draw()
        quiet = Self.mainCPU() - before < 1 ? quiet + 1 : 0
        if quiet >= 4, done() { return }
        try? await Task.sleep(for: .milliseconds(50))
      }
    }

    func close() {
      window.contentView = nil
      window.close()
    }

    /// CPU time of the main thread in milliseconds.
    static func mainCPU() -> Double {
      var now = timespec()
      clock_gettime(CLOCK_THREAD_CPUTIME_ID, &now)
      return Double(now.tv_sec) * 1000 + Double(now.tv_nsec) / 1e6
    }

    func views<T: NSView>(_ type: T.Type) -> [T] {
      window.contentView?.superview.map { Self.views(type, in: $0) } ?? []
    }

    static func views<T: NSView>(_ type: T.Type, in root: NSView) -> [T] {
      var found: [T] = []
      func visit(_ view: NSView) {
        if let view = view as? T { found.append(view) }
        view.subviews.forEach(visit)
      }
      visit(root)
      return found
    }
  }

  /// Times `work` on the main thread three times after `prepare`, prints the best and returns it.
  @discardableResult
  static func bench(
    _ name: String, prepare: () throws -> Void = {}, _ work: () throws -> Void
  ) rethrows -> (wall: Double, cpu: Double) {
    let clock = ContinuousClock()
    var best = (wall: Double.infinity, cpu: Double.infinity)
    for _ in 0..<3 {
      try prepare()
      let cpu = Offscreen.mainCPU()
      let wall = ScaleBench.milliseconds(try clock.measure(work))
      best = (min(best.wall, wall), min(best.cpu, Offscreen.mainCPU() - cpu))
    }
    ScaleBench.report(name, wall: best.wall, cpu: best.cpu)
    return best
  }

  @discardableResult
  static func bench(
    _ name: String, prepare: () async throws -> Void = {}, _ work: () async throws -> Void
  ) async rethrows -> (wall: Double, cpu: Double) {
    var best = (wall: Double.infinity, cpu: Double.infinity)
    for _ in 0..<3 {
      try await prepare()
      let start = ContinuousClock.now
      let cpu = Offscreen.mainCPU()
      try await work()
      best = (min(best.wall, ScaleBench.milliseconds(ContinuousClock.now - start)),
        min(best.cpu, Offscreen.mainCPU() - cpu))
    }
    ScaleBench.report(name, wall: best.wall, cpu: best.cpu)
    return best
  }

  /// The made-up project, open and listed, with the centers a window has.
  struct Opened {
    let project: ProjectStore
    let runs: RunCenter
    let records: RecordCenter
    let summary: SyntheticSummary
    let defaults: UserDefaults
    let domain: String

    func close() { defaults.removePersistentDomain(forName: domain) }

    func window(_ view: some View) -> some View {
      view.environment(project).environment(runs).environment(records).defaultAppStorage(defaults)
    }
  }

  private func opened() async throws -> Opened {
    let (root, summary) = try ScaleBench.project()
    let domain = "ScaleUIBenchmarkTests." + UUID().uuidString
    let defaults = try #require(UserDefaults(suiteName: domain))
    var keepDefaults = false
    defer { if !keepDefaults { defaults.removePersistentDomain(forName: domain) } }
    let project = ProjectStore(defaults: defaults)
    project.openProject(root)
    let deadline = ContinuousClock.now + .seconds(300)
    while project.status != .ready || project.runs.count != Int(summary.reports) {
      try #require(ContinuousClock.now < deadline, "Gave up waiting for the project to open")
      try await Task.sleep(for: .milliseconds(5))
    }
    keepDefaults = true
    return Opened(
      project: project, runs: RunCenter(defaults: defaults),
      records: RecordCenter(), summary: summary, defaults: defaults, domain: domain)
  }

  private func sidebar(_ screen: Offscreen) throws -> NSOutlineView {
    try #require(screen.views(NSOutlineView.self).first, "The sidebar is an outline")
  }

  // MARK: Sidebar

  @Test func sidebar() async throws {
    let opened = try await opened()
    defer { opened.close() }
    let project = opened.project
    let tests = project.items.flatMap(\.tests)
    var screen = Offscreen()
    defer { screen.close() }

    try Self.bench("app.ui.sidebar.build") {
      screen.close()
      screen = Offscreen()
      screen.show(opened.window(ProjectView(recordFirstTest: {})))
      screen.settle()
    }
    #expect(try sidebar(screen).numberOfRows > 0)

    // Open the project and its folders through their native disclosure controls, then close
    // them from the bottom up. The section header itself is not an expandable project row.
    let outline = try sidebar(screen)
    func click(_ row: Int) throws {
      outline.scrollRowToVisible(row)
      screen.draw()
      let rowView = try #require(outline.rowView(atRow: row, makeIfNecessary: true))
      let triangle = try #require(Offscreen.views(NSButton.self, in: rowView).first {
        $0.identifier == NSOutlineView.disclosureButtonIdentifier
      }, "Row \(row) has no native disclosure control")
      triangle.performClick(nil)
      screen.draw()
    }
    func setAll(expanded: Bool) throws -> Int {
      var clicks = 0
      var row = expanded ? 0 : outline.numberOfRows - 1
      while expanded ? row < outline.numberOfRows : row >= 0 {
        let item = outline.item(atRow: row)
        let sectionHeader = outline.rowView(atRow: row, makeIfNecessary: true)?.isGroupRowStyle == true
        if !sectionHeader, outline.isExpandable(item), outline.isItemExpanded(item) != expanded {
          try click(row)
          clicks += 1
          try #require(outline.isItemExpanded(outline.item(atRow: row)) == expanded,
            "The native disclosure control on row \(row) did not change its expanded state")
        }
        row += expanded ? 1 : -1
      }
      return clicks
    }
    var clicks = 0
    let opening = try Self.bench("app.ui.sidebar.expandAll", prepare: {
      _ = try setAll(expanded: false)
      screen.settle()
    }) {
      clicks = try setAll(expanded: true)
      try #require(clicks > 0, "The synthetic project must contain expandable project and folder rows")
      screen.settle()
    }
    ScaleBench.report("app.ui.sidebar.expandFolder", wall: opening.wall / Double(clicks), cpu: opening.cpu / Double(clicks))

    // The complete expanded tree includes its project/section, boards and New Board action.
    // Count those separately so a heading cannot stand in for a missing test row.
    func treeRows(_ items: [ProjectItem]) -> Int {
      items.reduce(0) { $0 + 1 + treeRows($1.children) }
    }
    try #require(tests.count >= 1_000)
    let fullRows = treeRows(project.items) + project.boards.names.count + 3
    try #require(outline.numberOfRows == fullRows)

    // From one test to another far from it, the editor opening on it included.
    let deep = try #require(tests.max { $0.id.split(separator: "/").count < $1.id.split(separator: "/").count })
    let near = try #require(tests.first)
    Self.bench("app.ui.sidebar.selectTest", prepare: {
      project.selection = near.id
      screen.settle()
    }) {
      project.selection = deep.id
      screen.settle()
    }
    project.selection = nil
    screen.settle()

    // A query typed letter by letter, then its results shown.
    let query = "checkout coupon"
    let expected = ProjectScanner.tests(in: project.items, matching: query)
    try #require(!expected.isEmpty && expected.count < tests.count)
    func type(_ text: String) throws {
      let field = try #require(screen.views(NSSearchField.self).first, "The sidebar has a search field")
      field.stringValue = text
      field.delegate?.controlTextDidChange?(Notification(name: NSControl.textDidChangeNotification, object: field))
    }
    let total = try await Self.bench("app.ui.sidebar.search.query", prepare: {
      try type("")
      await screen.settleAsync { (try? sidebar(screen).numberOfRows) == fullRows }
    }) {
      for end in 1...query.count {
        try type(String(query.prefix(end)))
        screen.draw()
      }
      await screen.settleAsync { (try? sidebar(screen).numberOfRows) == expected.count + 2 }
    }
    ScaleBench.report(
      "app.ui.sidebar.search.keystroke", wall: total.wall / Double(query.count), cpu: total.cpu / Double(query.count))
    let results = try sidebar(screen)
    // Only the section and project headings precede flat search results. Select every real
    // result through the native outline and require it to open the exact ranked test file.
    try #require(results.numberOfRows == expected.count + 2)
    for (index, item) in expected.enumerated() {
      results.selectRowIndexes(IndexSet(integer: index + 2), byExtendingSelection: false)
      await screen.settleAsync { project.selection == item.id && project.document != nil }
      try #require(project.selection == item.id)
      #expect(project.document?.url.standardizedFileURL == project.rootURL?.appending(path: item.id).standardizedFileURL)
    }
  }

  // MARK: Board

  @Test func board() async throws {
    let opened = try await opened()
    defer { opened.close() }
    let project = opened.project
    let allTests = try #require(project.boards.allTests)
    var screen = Offscreen()
    defer { screen.close() }

    // Until it first shows, then until what was worked out off the main thread is in it too.
    try Self.bench("app.ui.board.build") {
      screen.close()
      screen = Offscreen()
      project.showBoard(allTests)
      screen.show(opened.window(ProjectView(recordFirstTest: {})))
      screen.draw()
      screen.draw()
    }
    try Self.bench("app.ui.board.settle") {
      screen.settle(limit: .seconds(20))
    }

    let input = try #require(screen.views(BoardInput.InputView.self).first?.owner, "The board takes pans")
    try Self.bench("app.ui.board.pan") {
      for _ in 0..<10 {
        input.pan(CGSize(width: -80, height: -40))
        screen.draw()
      }
    }
    try Self.bench("app.ui.board.zoom") {
      for step in 0..<10 {
        input.zoom(step < 5 ? 1.25 : 0.8, CGPoint(x: 600, y: 380))
        screen.draw()
      }
    }

    let scene = project.boards.scene
    let source = BoardStatusSource(
      names: Dictionary(project.items.flatMap(\.tests).map { ($0.id, $0.name) }, uniquingKeysWith: { a, _ in a }),
      latest: project.latestRuns)
    try Self.bench("app.board.look") { _ = BoardLook(scene, source) }
  }

  // MARK: Test of 200 blocks

  @Test func largestTest() async throws {
    let opened = try await opened()
    defer { opened.close() }
    let project = opened.project
    let largest = opened.summary.largest
    let screen = Offscreen()
    defer { screen.close() }
    screen.show(opened.window(ProjectView(recordFirstTest: {})))
    screen.settle()

    let other = try #require(project.items.flatMap(\.tests).first { $0.id != largest })
    Self.bench("app.ui.flow.open.200", prepare: {
      project.selection = other.id
      screen.settle()
    }) {
      project.selection = largest
      screen.settle()
    }
    let document = try #require(project.document)
    #expect(document.order.count >= 200)

    let ids = document.order
    let first = try #require(ids.first)
    try Self.bench("app.ui.flow.select") {
      for id in ids.prefix(10) {
        document.selection = id
        screen.settle()
      }
    }

    var right = true
    try Self.bench("app.ui.flow.move") {
      document.move([.block(id: first)], dx: right ? 40 : -40, dy: 0, snap: false)
      right.toggle()
      screen.settle()
    }

    // A link taken away and made again.
    let link = try #require(document.layout.edges.first { $0.from != $0.to && $0.edge == "next" })
    try Self.bench("app.ui.flow.connect") {
      document.connect(link.from, edge: link.edge, to: nil)
      screen.settle()
      document.connect(link.from, edge: link.edge, to: link.to)
      screen.settle()
    }
    // What starting to drag a link out of a block works out: which blocks it may lead to.
    try Self.bench("app.flow.linkTargets") {
      _ = ids.filter { document.canConnect(link.from, edge: link.edge, to: $0) }
    }
    // What every change of the test, and every change of what it depends on, has checked.
    try Self.bench("app.flow.check") { document.revalidate() }
    // A letter typed into the test's name, as far as the main thread works it out at once, and
    // with what follows from it: the file written, the project told, the window drawn.
    let name = try #require(document.flow?.name)
    var letters = 0
    try Self.bench("app.flow.edit") {
      letters += 1
      document.updateFlow(field: "name") { $0.name = name + String(repeating: "x", count: letters) }
    }
    screen.settle()
    try Self.bench("app.ui.flow.edit") {
      letters += 1
      document.updateFlow(field: "name") { $0.name = name + String(repeating: "x", count: letters) }
      screen.settle()
    }
    document.updateFlow(field: "name") { $0.name = name }
    screen.settle()
    let flow = try #require(document.flow)
    try Self.bench("app.flow.lastRun") {
      _ = LastRun(file: largest, flow: flow, runs: project.runs, pinned: nil)
    }
    // What each step of dragging a block works out before the canvas draws it.
    try Self.bench("app.flow.dragFrame") {
      if let moved = moveOnCanvas(flow: flow, items: [.block(id: first)], dx: 40, dy: 0, snap: false) {
        _ = layoutFlow(flow: moved)
      }
    }
  }

  // MARK: Report of 10,000 steps

  /// The largest saved run, its first try made 10,000 steps long, in a folder of its own.
  private func longReport(_ project: ProjectStore) throws -> String {
    let steps = { (run: SavedRun) in run.report.attempts.map(\.steps.count).reduce(0, +) }
    let largest = try #require(project.runs.max { steps($0) < steps($1) })
    let data = try Data(contentsOf: URL(filePath: largest.dir).appending(path: "report.json"))
    var report = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
    var attempts = try #require(report["attempts"] as? [[String: Any]])
    let original = try #require(attempts.first?["steps"] as? [[String: Any]])
    attempts[0]["steps"] = (0..<10_000).map { index in
      var step = original[index % original.count]
      step["seq"] = index + 1
      return step
    }
    report["attempts"] = Array(attempts.prefix(1))
    let dir = FileManager.default.temporaryDirectory.appending(path: "jevision-scale-bench-app/LongReport")
    try? FileManager.default.removeItem(at: dir)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    try JSONSerialization.data(withJSONObject: report).write(to: dir.appending(path: "report.json"))
    return dir.path(percentEncoded: false)
  }

  @Test func longReport() async throws {
    let opened = try await opened()
    defer { opened.close() }
    let dir = try longReport(opened.project)
    #expect(try loadRun(dir: dir).report.attempts[0].steps.count == 10_000)
    var screen = Offscreen()
    defer { screen.close() }
    try await Self.bench("app.ui.report.open.10k") {
      screen.close()
      screen = Offscreen()
      screen.show(opened.window(RunReportView(dir: dir)))
      await screen.settleAsync { !screen.views(NSScrollView.self).isEmpty }
      try #require(!screen.views(NSScrollView.self).isEmpty, "The loaded report must replace its progress indicator")
    }
  }
}
