import AppKit
import Foundation
import JEVisionCore
import Testing

@testable import JEVision

private func node(
  _ id: String, _ kind: BoardNodeKind, x: Double, y: Double, width: Double = 200, height: Double = 80, open: Bool = false
) -> BoardNode {
  BoardNode(id: id, kind: kind, rect: Rect(x: x, y: y, width: width, height: height), color: "", open: open, extra: "")
}

private func board(_ nodes: [BoardNode], edges: [BoardEdge] = []) -> Board {
  Board(nodes: nodes, edges: edges, allTests: false, extra: "")
}

private func note(_ id: String, x: Double, y: Double) -> BoardNode {
  node(id, .text(text: id), x: x, y: y)
}

private func arrow(_ id: String, _ from: String, _ to: String) -> BoardEdge {
  BoardEdge(
    id: id, from: from, fromSide: "right", to: to, toSide: "left", fromArrow: false, toArrow: true, label: "", color: "",
    extra: "")
}

struct BoardSceneTests {
  @Test func asksOnlyForWhatIsInView() {
    let scene = BoardScene(
      board([note("a", x: 0, y: 0), note("b", x: 5000, y: 0), note("c", x: 10000, y: 0)], edges: [arrow("ab", "a", "b")]))
    let ids = { (visible: BoardScene.Visible) in visible.nodes.map { scene.layout.nodes[$0].id } }
    #expect(ids(scene.visible(in: CGRect(x: -10, y: -10, width: 500, height: 500))) == ["a"])
    #expect(ids(scene.visible(in: CGRect(x: 4900, y: 0, width: 400, height: 100))) == ["b"])
    #expect(scene.visible(in: CGRect(x: 2000, y: 0, width: 400, height: 100)).nodes.isEmpty)
    // The arrow between the two shows wherever it passes, though neither end is in view.
    #expect(scene.visible(in: CGRect(x: 2400, y: 20, width: 100, height: 100)).edges.count == 1)
    #expect(scene.visible(in: CGRect(x: 2400, y: 3000, width: 100, height: 100)).edges.isEmpty)
  }

  @Test func holdsNodesDraggedAwayWhereTheyAreHeld() {
    let scene = BoardScene(board([note("a", x: 0, y: 0), note("b", x: 5000, y: 0)], edges: [arrow("ab", "a", "b")]))
    let shift = CGSize(width: 8000, height: 0)
    let near = CGRect(x: 7900, y: 0, width: 400, height: 100)
    let drawn = scene.visible(in: near, moved: ["a"], by: shift)
    #expect(drawn.nodes.map { scene.layout.nodes[$0].id } == ["a"])
    #expect(drawn.edges.count == 1, "the arrows of a node being dragged go with it")
    let left = scene.visible(in: CGRect(x: -10, y: -10, width: 300, height: 100), moved: ["a"], by: shift)
    #expect(left.nodes.isEmpty, "a node held elsewhere isn't where the layout has it")
  }

  @Test func findsTheNodeUnderAPointAndMovesBetweenNodes() {
    let frame = node("f", .group(label: "Frame", collapsed: false, folder: nil), x: 0, y: 0, width: 600, height: 400)
    let scene = BoardScene(
      board([frame, node("n", .text(text: "n"), x: 50, y: 150), note("right", x: 700, y: 150), note("below", x: 50, y: 600)]))
    #expect(scene.hit(CGPoint(x: 100, y: 10), header: 40, wholeFrames: false) == "f")
    #expect(scene.hit(CGPoint(x: 400, y: 300), header: 40, wholeFrames: false) == nil, "inside a frame a drag pans")
    #expect(scene.hit(CGPoint(x: 400, y: 300), header: 40, wholeFrames: true) == "f")
    #expect(scene.hit(CGPoint(x: 100, y: 180), header: 40, wholeFrames: false) == "n", "what is on a frame wins")
    #expect(scene.hit(CGPoint(x: 9000, y: 9000), header: 40, wholeFrames: true) == nil)

    #expect(scene.neighbour(of: "n", .right) == "right")
    #expect(scene.neighbour(of: "n", .down) == "below")
    #expect(scene.neighbour(of: "right", .right) == nil)
    #expect(scene.neighbour(of: "missing", .up) == nil)
    let middle = CGPoint(x: 800, y: 190)
    #expect(scene.nearest(to: middle, in: CGRect(x: -100, y: -100, width: 2000, height: 1000)) == "right")
    #expect(scene.nearest(to: middle, in: CGRect(x: 5000, y: 5000, width: 10, height: 10)) == nil)
  }

  @Test func knowsTheTestsInsideAFoldedFrameAndThePictures() {
    let frame = node("f", .group(label: "Smoke", collapsed: true, folder: nil), x: 0, y: 0, width: 800, height: 400)
    let scene = BoardScene(
      board([
        frame, node("t1", .file(file: "a.flow.json"), x: 40, y: 80), node("t2", .file(file: "b.flow.json"), x: 300, y: 80),
        node("out", .file(file: "c.flow.json"), x: 1200, y: 80),
        node("pic", .file(file: "boards/images/Shot.png"), x: 0, y: 900),
        node("doc", .file(file: "notes/readme.md"), x: 300, y: 900),
      ]))
    #expect(Set(scene.frameTests["f"] ?? []) == ["a.flow.json", "b.flow.json"])
    #expect(scene.places["t1"] == nil, "a fold hides the cards inside")
    #expect(scene.nodes["t1"] != nil, "but the board still has them")
    #expect(scene.images == ["pic"])
    #expect(scene.places["out"] != nil)
  }

  @Test func routesArrowsOnlyAmongTheCardsNearThem() {
    func bounds(_ extra: [String: CGRect]) -> [String: CGRect] {
      ["a": CGRect(x: 0, y: 0, width: 100, height: 100), "b": CGRect(x: 1000, y: 0, width: 100, height: 100)]
        .merging(extra) { $1 }
    }
    let uses = { (file: String, target: String) in
      MapTest(file: file, name: file, blocks: 1, uses: [target], errors: 0, readable: true, flaky: false)
    }
    let map = ["a": uses("a", "b"), "b": uses("b", "a")]
    let alone = ProjectMap.links(map, bounds: bounds([:]))
    let crowd = ProjectMap.links(map, bounds: bounds(["far": CGRect(x: 90000, y: 0, width: 100, height: 100)]))
    #expect(alone.count == 2)
    #expect(alone == crowd, "a card too far away to matter changes nothing")

    let wall = ProjectMap.links(map, bounds: bounds(["wall": CGRect(x: 500, y: -200, width: 40, height: 500)]))
    #expect(wall != alone, "a card in the way makes the arrow go around")
  }

  @Test func routesThousandsOfCardsWithoutGoingOverAllOfThem() {
    var bounds: [String: CGRect] = [:]
    var map: [String: MapTest] = [:]
    for i in 0..<2000 {
      let file = "t\(i)"
      bounds[file] = CGRect(x: Double(i % 50) * 300, y: Double(i / 50) * 200, width: 224, height: 96)
      map[file] = MapTest(
        file: file, name: file, blocks: 1, uses: i < 1999 ? ["t\(i + 1)"] : [], errors: 0, readable: true, flaky: false)
    }
    #expect(ProjectMap.links(map, bounds: bounds).count == 1999)
  }
}

struct BoardStatusTests {
  private func status(
    _ info: MapTest? = nil, running: Bool = false, waiting: Bool = false, missing: Bool = false
  ) -> BoardTestStatus.Kind {
    BoardTestStatus(info: info, last: nil, isRunning: running, isWaiting: waiting, isMissing: missing).kind
  }

  private func info(readable: Bool = true, errors: UInt32 = 0) -> MapTest {
    MapTest(file: "a", name: "A", blocks: 2, uses: [], errors: errors, readable: readable, flaky: false)
  }

  @Test func ranksWhatStopsATestAheadOfHowItLastRan() {
    #expect(status(missing: true) == .missing)
    #expect(status(running: true, waiting: true) == .running)
    #expect(status(info(readable: false), running: true) == .running)
    #expect(status(info(readable: false), waiting: true) == .waiting)
    #expect(status(info(readable: false)) == .unreadable)
    #expect(status(info(errors: 2)) == .broken)
    #expect(status(info()) == .notRun)
    #expect(status() == .notRun)
  }

  @Test func showsEachStatusByShapeAndOnlyTheOnesWorthItByColor() {
    let kinds: [BoardTestStatus.Kind] = [
      .missing, .unreadable, .broken, .running, .waiting, .notRun, .ran(.passed), .ran(.failed), .ran(.flaky),
    ]
    let symbols = kinds.map { BoardTestStatus($0).symbol }
    #expect(Set(symbols).count == kinds.count, "no two statuses share a shape")
    #expect(BoardTestStatus(.waiting).tint == nil)
    #expect(BoardTestStatus(.notRun).tint == nil)
    #expect(BoardTestStatus(.running).tint != nil)
    #expect(BoardTestStatus(.ran(.failed)).tint != nil)
    #expect(kinds.allSatisfy { !BoardTestStatus($0).text.isEmpty })
  }

  @Test func sumsUpTheTestsInAFrameAndLeadsWithTheWorst() {
    let rollup = BoardRollup([
      BoardTestStatus(.ran(.passed)), BoardTestStatus(.ran(.passed)), BoardTestStatus(.ran(.failed)),
      BoardTestStatus(.notRun),
    ])
    #expect((rollup.tests, rollup.passed, rollup.failed, rollup.running) == (4, 2, 1, 0))
    #expect(rollup.lead?.kind == .ran(.failed))
    #expect(rollup.text.contains("2") && rollup.text.contains("4"))

    #expect(BoardRollup([BoardTestStatus(.ran(.failed)), BoardTestStatus(.running)]).lead?.kind == .running)
    #expect(BoardRollup([BoardTestStatus(.ran(.passed)), BoardTestStatus(.waiting)]).lead?.kind == .waiting)
    #expect(BoardRollup([BoardTestStatus(.ran(.passed))]).lead?.kind == .ran(.passed))
    #expect(BoardRollup([]).text.isEmpty)
    #expect(BoardRollup([]).lead == nil)
  }

  @Test func looksAtTheBoardOnceForEveryFrameOfAPan() {
    let scene = BoardScene(
      board([
        node("f", .group(label: "Smoke", collapsed: false, folder: nil), x: 0, y: 0, width: 800, height: 400),
        node("t1", .file(file: "a.flow.json"), x: 40, y: 80), node("t2", .file(file: "gone.flow.json"), x: 300, y: 80),
        node("again", .file(file: "a.flow.json"), x: 1200, y: 80),
        node("pic", .file(file: "boards/images/Shot.png"), x: 0, y: 900), note("n", x: 300, y: 900),
      ]))
    var source = BoardStatusSource(names: ["a.flow.json": "A"], map: [:], latest: [:])
    let still = BoardLook(scene, source)
    #expect(still.tests == ["a.flow.json"], "only the project's tests, each once")
    #expect(still.status["a.flow.json"]?.kind == .notRun)
    #expect(still.rollups["f"]?.tests == 1, "a test the project no longer has isn't counted")
    #expect(still.glances.count == scene.layout.nodes.count)
    let glance = { (id: String) in still.glances[scene.places[id] ?? 0] }
    #expect(glance("f").shape == .frame)
    #expect(glance("t1").shape == .card && glance("t1").title == "A")
    #expect(glance("pic").shape == .picture && glance("pic").title == "Shot.png")
    #expect(glance("n").shape == .note)

    source.running = "a.flow.json"
    let running = BoardLook(scene, source)
    #expect(running != still)
    #expect(running.status["a.flow.json"]?.kind == .running)
    #expect(running.rollups["f"]?.running == 1)
    #expect(running.glances[scene.places["again"] ?? 0].badge?.kind == .running, "every card of the test shows it")

    source.running = nil
    source.waiting = ["a.flow.json"]
    #expect(BoardLook(scene, source).status["a.flow.json"]?.kind == .waiting)
  }
}

struct BoardBlocksTests {
  private func layout(width: Double = 300, height: Double = 100) -> FlowLayout {
    FlowLayout(
      width: width, height: height,
      blocks: [
        LaidBlock(id: "s0", rect: Rect(x: 0, y: 0, width: 100, height: 60)),
        LaidBlock(id: "s1", rect: Rect(x: 200, y: 40, width: 100, height: 60)),
      ], frames: [], edges: [], slots: [], ports: [])
  }

  @Test func showsMoreOfABlockTheLargerItComesOut() {
    #expect(BoardBlockDetail(scale: 0.05) == .shape)
    #expect(BoardBlockDetail(scale: 0.149) == .shape)
    #expect(BoardBlockDetail(scale: 0.15) == .title)
    #expect(BoardBlockDetail(scale: 0.399) == .title)
    #expect(BoardBlockDetail(scale: 0.4) == .summary)
    #expect(BoardBlockDetail(scale: 2) == .summary)
  }

  @Test func fitsTheBlocksIntoTheCardAndFindsTheOneUnderAPoint() throws {
    let card = CGRect(x: 1000, y: 500, width: 600, height: 400)
    let flow = layout()
    let geometry = try #require(BoardBlockGeometry(card: card, layout: flow))
    #expect(card.contains(geometry.area))
    #expect(geometry.fit > 0)
    #expect(abs(geometry.area.width - flow.width * geometry.fit) < 0.001)

    let second = try #require(flow.blocks.last)
    let rect = geometry.rect(of: second.rect)
    #expect(abs(rect.width - second.rect.width * geometry.fit) < 0.001)
    #expect(geometry.block(at: CGPoint(x: rect.midX, y: rect.midY), in: flow) == "s1")
    let first = geometry.rect(of: flow.blocks[0].rect)
    #expect(geometry.block(at: CGPoint(x: first.midX, y: first.midY), in: flow) == "s0")
    #expect(geometry.block(at: CGPoint(x: card.minX - 5, y: card.minY - 5), in: flow) == nil)
    #expect(geometry.block(at: CGPoint(x: first.maxX + 1, y: first.maxY + 1), in: flow) == nil, "the gap between blocks")
  }

  @Test func showsNothingForATestWithoutBlocks() {
    let card = CGRect(x: 0, y: 0, width: 600, height: 400)
    #expect(BoardBlockGeometry(card: card, layout: layout(width: 0, height: 0)) == nil)
  }

  @Test func countsHowTheBlocksOfATestDid() {
    #expect(BoardBlockRun().text(total: 5).isEmpty)
    let run = BoardBlockRun(marks: ["s0": .passed, "s1": .failed, "s2": .passed, "s3": .skipped], current: nil)
    let text = run.text(total: 5)
    #expect(text.contains("2") && text.contains("5") && text.contains("1"))
    #expect(BoardBlockRun(marks: ["s0": .passed], current: "s1").text(total: 2).contains("1"))
  }
}

@MainActor
struct BoardPicturesTests {
  private func project() throws -> (ProjectBoards, URL) {
    let folder = FileManager.default.temporaryDirectory.appending(path: "board-pictures-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let defaults = try #require(UserDefaults(suiteName: "jevision-tests-\(UUID().uuidString)"))
    let boards = ProjectBoards(defaults: defaults)
    boards.open(folder)
    return (boards, folder)
  }

  private func png(width: Int = 40, height: Int = 20) throws -> Data {
    let image = try #require(
      NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8, samplesPerPixel: 4,
        hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0))
    return try #require(image.representation(using: .png, properties: [:]))
  }

  private func undoManager() -> UndoManager {
    let undo = UndoManager()
    undo.groupsByEvent = false
    return undo
  }

  @Test func keepsAPictureInTheProjectAndPutsItOnTheBoard() throws {
    let (boards, folder) = try project()
    boards.sync(tests: ["A.flow.json"], rootTitle: "P")
    let undo = undoManager()
    undo.beginUndoGrouping()
    let id = try #require(try boards.addImage(try png(), named: "Shot.png", at: CGPoint(x: 400, y: 300), undoManager: undo))
    undo.endUndoGrouping()

    #expect(FileManager.default.fileExists(atPath: folder.appending(path: "boards/images/Shot.png").path))
    #expect(boards.scene.images.contains(id))
    let added = try #require(boards.scene.nodes[id])
    #expect(added.kind == .file(file: "boards/images/Shot.png"))
    #expect(added.rect.width > 0 && added.rect.height > 0)
    #expect(abs(added.rect.width / added.rect.height - 2) < 0.05, "the picture keeps its proportions")

    let second = try #require(try boards.addImage(try png(), named: "Shot.png", at: .zero, undoManager: nil))
    #expect(boards.scene.nodes[second]?.kind == .file(file: "boards/images/Shot 2.png"), "a name that is taken gets a number")

    undo.undo()
    #expect(boards.scene.nodes[id] == nil)
  }

  @Test func refusesWhatIsNotAPictureInWordsOfItsOwn() throws {
    let (boards, folder) = try project()
    boards.sync(tests: ["A.flow.json"], rootTitle: "P")
    let before = boards.board
    do {
      try boards.addImage(Data("not a picture".utf8), named: "Shot.png", at: .zero, undoManager: nil)
      Issue.record("garbage was taken for a picture")
    } catch {
      #expect(!ProjectBoards.message(error).isEmpty)
      #expect(!ProjectBoards.message(error).contains(String(localized: "This board can’t be read: ")))
    }
    do {
      try boards.addImage(try png(), named: "notes.txt", at: .zero, undoManager: nil)
      Issue.record("a picture named like a text was taken")
    } catch BoardError.NotAPicture {}
    #expect(boards.board == before)
    #expect(!FileManager.default.fileExists(atPath: folder.appending(path: "boards/images/notes.txt").path))
  }

  @Test func opensATestOnTheBoardToShowItsBlocksAndFoldsThemBack() throws {
    let (boards, folder) = try project()
    let click = #""type":"click","target":{"point":{"x":0.5,"y":0.5}},"button":"left""#
    try """
      {"format":1,"name":"A","app":"com.example.App","start":"s0","blocks":{"s0":{\(click),"next":"s1"},"s1":{\(click)}}}
      """.write(to: folder.appending(path: "A.flow.json"), atomically: true, encoding: .utf8)
    boards.sync(tests: ["A.flow.json"], rootTitle: "P")
    let card = try #require(boards.board?.nodes.first { BoardLook.test($0) == "A.flow.json" })
    #expect(!card.open)

    try boards.toggleBlocks(of: card.id, undoManager: nil)
    let open = try #require(boards.scene.nodes[card.id])
    #expect(open.open)
    #expect(open.rect.height > card.rect.height, "the card grows to hold its blocks")
    #expect(open.rect.x == card.rect.x && open.rect.y == card.rect.y)

    try boards.toggleBlocks(of: card.id, undoManager: nil)
    let closed = try #require(boards.scene.nodes[card.id])
    #expect(!closed.open)
    #expect(closed.rect == card.rect)
  }
}
