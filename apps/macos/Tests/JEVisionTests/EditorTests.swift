import Foundation
import JEVisionCore
import Testing

@testable import JEVision

struct BlockSummaryTests {
  private func summary(_ kind: BlockKind) -> String {
    BlockSummary.text(kind, appName: { $0 == "com.apple.Mail" ? "Mail" : $0 })
  }

  @Test func describesWhatABlockDoes() {
    #expect(summary(.openApp(app: "com.apple.Mail", next: nil)) == "Mail")
    #expect(summary(.openApp(app: nil, next: nil)) == "The app under test")
    #expect(
      summary(.type(text: "${secret.ACME_PASSWORD}", into: .image("refs/password-field.png"), next: nil))
        == "Type secret ACME_PASSWORD into “password field”")
    #expect(summary(.type(text: "alice@example.com", into: nil, next: nil)) == "Type “alice@example.com”")
    #expect(
      summary(.click(target: .image("refs/sign_in-button.png"), button: .left, double: false, waitForChange: false, next: nil))
        == "Click “sign in button”")
    #expect(
      summary(.click(target: .image(""), button: .left, double: false, waitForChange: false, next: nil))
        == "Choose what to click")
    #expect(summary(.pressKeys(keys: "cmd+shift+s", next: nil)) == "Press ⇧⌘S")
    #expect(summary(.openFile(files: ["docs/a.txt", "b.txt", "c.txt"], app: nil, newInstance: false, closeAfter: false, next: nil)) == "a.txt and 2 more")
    #expect(summary(.runTest(test: "shared/sign-in.flow.json", inputs: [:], next: nil)) == "Runs “sign-in”")
  }

  @Test func neverSplitsADataToken() {
    let shown = summary(.type(text: "Hello, ${data.name}!", into: nil, next: nil))
    #expect(shown == "Type “Hello, " + unbreakablePlaceholders(text: "${data.name}") + "!”")
    #expect(shown.replacing("\u{2060}", with: "") == "Type “Hello, ${data.name}!”")
    #expect(BlockSummary.quoted("Dear customer, ${data.full_name}") == "“Dear customer,…”", "Cut before the token")
  }

  @Test func namesEveryWayOutOfAnIf() {
    let plain = BlockKind.if(reference: Reference(image: "a.png", compare: .exact, area: nil, ignore: [], hide: []), then: nil, elseIf: [], otherwise: nil)
    #expect(summary(plain) == "If “a” is on screen")
    #expect(plain.branchName("then") == "Yes")
    #expect(plain.branchName("else") == "No")
    #expect(plain.branchName("elseIf1") == nil)
    let three = addIfCondition(kind: addIfCondition(kind: plain))
    #expect(summary(three) == "If “a” is on screen · or 2 more")
    #expect(["then", "elseIf1", "elseIf2", "else"].map { three.branchName($0) } == ["Condition 1", "Condition 2", "Condition 3", "Otherwise"])
    #expect(three.branchName("elseIf3") == nil)
    #expect(three.branchName("next") == nil)
    #expect(BlockKind.check(reference: Reference(image: "", compare: .exact, area: nil, ignore: [], hide: []), next: nil).branchName("then") == nil)
  }

  @Test func shortensLongText() {
    #expect(BlockSummary.quoted("short") == "“short”")
    #expect(BlockSummary.quoted("- milk\n\tok\r\n") == "“- milk⏎⇥ok⏎”")
    #expect(BlockSummary.quoted(String(repeating: "a", count: 30)) == "“" + String(repeating: "a", count: 23) + "…”")
    #expect(BlockSummary.secretName("${secret.X_1}") == "X_1")
    #expect(BlockSummary.secretName("pass ${secret.X}") == nil)
  }
}

struct KeyComboTests {
  @Test func showsCombinationsTheWayMenusDo() {
    #expect(KeyCombo.display("cmd+shift+s") == "⇧⌘S")
    #expect(KeyCombo.display("ctrl+option+return") == "⌃⌥↩")
    #expect(KeyCombo.display("f5") == "F5")
  }

  @Test func recordsKeysInFileOrder() {
    #expect(KeyCombo.from(keyCode: 1, modifierFlags: [.shift, .command]) == "cmd+shift+s")
    #expect(KeyCombo.from(keyCode: 36, modifierFlags: []) == "return")
    #expect(KeyCombo.from(keyCode: 76, modifierFlags: []) == "return")
    #expect(KeyCombo.from(keyCode: 96, modifierFlags: [.control]) == "ctrl+f5")
    #expect(KeyCombo.from(keyCode: 10, modifierFlags: []) == nil)
  }

  /// On a Russian layout ⌘S types "ы"; the shortcut is still recorded as the key's place.
  @Test func recordsShortcutsTheSameOnEveryLayout() {
    #expect(KeyCombo.from(keyCode: 1, modifierFlags: [.command]) == "cmd+s")
    #expect(KeyCombo.from(keyCode: 45, modifierFlags: [.command]) == "cmd+n")
    #expect(KeyCombo.from(keyCode: 43, modifierFlags: [.command]) == "cmd+,")  }
}

struct RelativePathTests {
  @Test func writesPathsFromTheTestFolder() {
    let tests = URL(filePath: "/p/tests")
    #expect(RelativePath.from(tests, to: URL(filePath: "/p/tests/refs/a.png")) == "refs/a.png")
    #expect(RelativePath.from(tests, to: URL(filePath: "/p/data/users.csv")) == "../data/users.csv")
    #expect(RelativePath.resolve("../data/users.csv", in: tests).path == "/p/data/users.csv")
    #expect(RelativePath.isInside(URL(filePath: "/p/data/users.csv"), folder: URL(filePath: "/p")))
    #expect(!RelativePath.isInside(URL(filePath: "/other/users.csv"), folder: URL(filePath: "/p")))
    #expect(!RelativePath.isInside(URL(filePath: "/pp/users.csv"), folder: URL(filePath: "/p")))
  }

  @Test func picksTheImageForTheAppearanceAsRunsDo() throws {
    let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: folder) }
    try FileManager.default.createDirectory(at: folder.appending(path: "refs"), withIntermediateDirectories: true)
    let dark = folder.appending(path: "refs/x.dark.png")
    try Data().write(to: dark)

    // Only the dark picture: a reference has it, a picture runs need in light mode doesn't.
    #expect(AppearanceImage.url("refs/x.png", in: folder, dark: true) == dark)
    #expect(AppearanceImage.url("refs/x.png", in: folder, dark: false) == nil)
    #expect(AppearanceImage.url("refs/x.png", in: folder, dark: false, variants: true) == dark)

    let light = folder.appending(path: "refs/x.png")
    try Data().write(to: light)
    #expect(AppearanceImage.url("refs/x.png", in: folder, dark: true) == dark)
    #expect(AppearanceImage.url("refs/x.png", in: folder, dark: false, variants: true) == light)
    try FileManager.default.removeItem(at: dark)
    #expect(AppearanceImage.url("refs/x.png", in: folder, dark: true) == light)
    #expect(AppearanceImage.url("", in: folder, dark: true) == nil)
  }

  @Test func namesNewTestFiles() {
    #expect(RelativePath.testFileName(" Sign in: valid / invalid ") == "Sign in- valid - invalid.flow.json")
    #expect(RelativePath.testFileName("  ") == "Untitled.flow.json")
    let taken: Set = ["Login", "Login 2"]
    #expect(RelativePath.unique("Login") { taken.contains($0) } == "Login 3")
    #expect(RelativePath.unique("Logout") { taken.contains($0) } == "Logout")
  }
}

@MainActor
struct FlowDocumentTests {
  private static let chain = """
    {"format": 1, "name": "T", "app": "com.apple.TextEdit", "start": "a", "blocks": {
      "a": {"type": "pressKeys", "keys": "a", "next": "b"},
      "b": {"type": "pressKeys", "keys": "b"}}}
    """

  private func file(_ text: String) throws -> URL {
    let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let url = folder.appending(path: "t.flow.json")
    try text.write(to: url, atomically: true, encoding: .utf8)
    return url
  }

  private func nextOf(_ document: FlowDocument, _ id: String) -> String? {
    if case .pressKeys(_, let next) = document.block(id)?.kind { return next }
    if case .click(_, _, _, _, let next) = document.block(id)?.kind { return next }
    return nil
  }

  @Test func insertsSavesAndUndoes() throws {
    let url = try file(Self.chain)
    let document = FlowDocument(url: url)
    let undo = UndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    #expect(document.order == ["a", "b"])

    undo.beginUndoGrouping()
    document.insert("click", at: Slot(from: "a", edge: "next"))
    undo.endUndoGrouping()
    #expect(document.selection == "click")
    #expect(document.order == ["a", "click", "b"])
    #expect(nextOf(document, "click") == "b")
    #expect(try String(contentsOf: url, encoding: .utf8).contains("\"click\""))

    undo.beginUndoGrouping()
    document.delete("click")
    undo.endUndoGrouping()
    #expect(document.order == ["a", "b"])
    #expect(document.selection == "b")

    undo.undo()
    #expect(document.order == ["a", "click", "b"])
    undo.undo()
    #expect(document.order == ["a", "b"])
    #expect(document.selection == "a" || document.selection == nil)
    undo.redo()
    #expect(document.order == ["a", "click", "b"])
  }

  @Test func typingIntoOneFieldUndoesAsOneStep() throws {
    let document = FlowDocument(url: try file(Self.chain))
    let undo = CountingUndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    document.selection = "a"
    undo.beginUndoGrouping()
    for keys in ["x", "cmd+x", "cmd+shift+x"] {
      document.update("a", field: "keys") { $0 = .pressKeys(keys: keys, next: "b") }
    }
    document.update("b", field: "keys") { $0 = .pressKeys(keys: "y", next: nil) }
    undo.endUndoGrouping()
    #expect(undo.registrations == 2)
    undo.undo()
    #expect(document.block("a")?.kind == .pressKeys(keys: "a", next: "b"))
    #expect(document.block("b")?.kind == .pressKeys(keys: "b", next: nil))
  }

  @Test func movesTheSelectionTogetherAndUndoes() throws {
    let url = try file(Self.chain)
    let document = FlowDocument(url: url)
    let undo = UndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    let before = document.layout.blocks
    document.pick([.block(id: "a"), .block(id: "b")], prefer: "b")
    #expect(document.selection == "b")
    #expect(document.pickedInOrder == [.block(id: "a"), .block(id: "b")])

    undo.beginUndoGrouping()
    document.move(document.pickedInOrder, dx: 165, dy: 0)
    undo.endUndoGrouping()
    let positions = try #require(document.flow?.canvas.positions)
    #expect(positions.count == 2)
    for laid in before {
      let moved = try #require(positions[laid.id])
      #expect(moved.x.truncatingRemainder(dividingBy: FlowCanvas.grid) == 0)
      #expect(abs(moved.x - (laid.rect.x + 165)) <= FlowCanvas.grid)
    }
    #expect(document.picked.count == 2, "moving keeps the selection")
    #expect(try String(contentsOf: url, encoding: .utf8).contains("\"canvas\""))

    undo.undo()
    #expect(document.flow?.canvas.positions.isEmpty == true)
    #expect(document.layout.blocks == before)
  }

  @Test func linksAndUnlinksBlocksButMakesNoLoops() throws {
    let document = FlowDocument(url: try file(Self.chain))
    let undo = UndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    #expect(!document.canConnect("b", edge: "next", to: "a"), "b → a would loop back")
    #expect(!document.canConnect("a", edge: "next", to: "b"), "a already leads to b")
    #expect(!document.canConnect("a", edge: "then", to: "b"), "Press Keys has no Yes branch")

    undo.beginUndoGrouping()
    document.connect("a", edge: "next", to: nil)
    undo.endUndoGrouping()
    #expect(nextOf(document, "a") == nil)
    #expect(document.canConnect("a", edge: "next", to: "b"))
    #expect(document.canConnect("b", edge: "next", to: "a"))
    undo.undo()
    #expect(nextOf(document, "a") == "b")
  }

  @Test func duplicatesAfterTheSelectedBlockAndPastesIntoAnotherTest() throws {
    let document = FlowDocument(url: try file(Self.chain))
    document.undoManager = UndoManager()
    document.pick([.block(id: "a")])
    document.duplicate()
    #expect(document.order == ["a", "a2", "b"])
    #expect(nextOf(document, "a") == "a2")
    #expect(nextOf(document, "a2") == "b")
    #expect(document.selection == "a2")
    #expect(document.picked == [.block(id: "a2")])

    document.pick([.block(id: "a2"), .block(id: "b")])
    let copied = document.copyPicked()
    let other = FlowDocument(url: try file(Self.chain))
    other.pick([])
    #expect(other.paste(copied))
    #expect(other.flow?.blocks.count == 4)
    #expect(other.picked.count == 2)
    #expect(!other.paste("not a test"))
    #expect(other.flow?.blocks.count == 4)
  }

  @Test func addsEditsAndDeletesNotesAndFrames() throws {
    let url = try file(Self.chain)
    let document = FlowDocument(url: url)
    let undo = CountingUndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    undo.beginUndoGrouping()
    document.addNote(at: Point(x: 600, y: 40), author: "Anna")
    for text in ["C", "Ch", "Check twice"] {
      let id = try #require(document.flow?.canvas.notes.first?.id)
      document.editNote(id, text: text)
    }
    undo.endUndoGrouping()
    #expect(undo.registrations == 2, "adding is one step, typing another")
    let note = try #require(document.flow?.canvas.notes.first)
    #expect(note.text == "Check twice")
    #expect(note.author == "Anna")
    #expect(!note.time.isEmpty)
    #expect(document.picked == [.note(id: note.id)])
    #expect(document.selection == nil)

    undo.beginUndoGrouping()
    document.addFrame(Rect(x: 0, y: 0, width: 400, height: 300))
    let frame = try #require(document.flow?.canvas.sections.first)
    document.editFrame(frame.id, title: "Sign in")
    document.resize(.section(id: frame.id), to: Rect(x: 0, y: 0, width: 500, height: 300))
    undo.endUndoGrouping()
    #expect(document.flow?.canvas.sections.first?.title == "Sign in")
    #expect(document.flow?.canvas.sections.first?.rect.width == 496)
    #expect(try String(contentsOf: url, encoding: .utf8).contains("Sign in"))

    document.pick([.note(id: note.id), .section(id: frame.id)])
    undo.beginUndoGrouping()
    document.remove(document.pickedInOrder)
    undo.endUndoGrouping()
    #expect(document.flow?.canvas.notes.isEmpty == true)
    #expect(document.flow?.canvas.sections.isEmpty == true)
    #expect(document.picked.isEmpty)
    #expect(document.order == ["a", "b"])
    undo.undo()
    #expect(document.flow?.canvas.notes.count == 1)
    #expect(document.flow?.canvas.sections.count == 1)
  }

  @Test func reloadsChangesMadeElsewhereKeepingTheSelection() throws {
    let url = try file(Self.chain)
    let document = FlowDocument(url: url)
    document.selection = "b"
    try Self.chain.replacing("\"keys\": \"a\"", with: "\"keys\": \"z\"").write(to: url, atomically: true, encoding: .utf8)
    document.reload()
    #expect(document.block("a")?.kind == .pressKeys(keys: "z", next: "b"))
    #expect(document.selection == "b")

    try "{ broken".write(to: url, atomically: true, encoding: .utf8)
    document.reload()
    #expect(document.flow == nil)
    #expect(document.failure?.isEmpty == false)
  }
}

@MainActor
struct CanvasKeyboardTests {
  /// Open, then a check: Yes types, No presses keys; both go on to the end.
  private static let branches = """
    {"format": 1, "name": "T", "app": "com.apple.TextEdit", "start": "open", "blocks": {
      "open": {"type": "pressKeys", "keys": "a", "next": "if"},
      "if": {"type": "if", "reference": {"image": "a.png"}, "then": "yes", "else": "no"},
      "yes": {"type": "type", "text": "y", "next": "end"},
      "no": {"type": "pressKeys", "keys": "n", "next": "end"},
      "end": {"type": "pressKeys", "keys": "e"}}}
    """

  private func document() throws -> FlowDocument {
    let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let url = folder.appending(path: "t.flow.json")
    try Self.branches.write(to: url, atomically: true, encoding: .utf8)
    return FlowDocument(url: url)
  }

  @Test func arrowsFollowTheLinks() throws {
    let document = try document()
    func go(_ from: String?, _ direction: CanvasDirection) -> String? {
      CanvasNavigation.next(from: from, toward: direction, in: document.layout, order: document.order)
    }
    #expect(go(nil, .down) == "open")
    #expect(go(nil, .up) == document.order.last)
    #expect(go("open", .down) == "if")
    #expect(go("if", .down) == "yes", "Down takes the Yes branch first")
    #expect(go("yes", .right) == "no")
    #expect(go("no", .left) == "yes")
    #expect(go("no", .up) == "if")
    #expect(go("yes", .down) == "end")
    #expect(go("end", .up) == "no", "Of two steps leading here, the one read last")
    #expect(go("open", .up) == nil)
  }

  @Test func linksFromTheKeyboardAndUndoes() throws {
    let document = try document()
    let undo = UndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    document.selection = "yes"

    undo.beginUndoGrouping()
    document.unlink([Slot(from: "yes", edge: "next")])
    undo.endUndoGrouping()
    #expect(document.block("yes")?.kind == .type(text: "y", into: nil, next: nil))

    #expect(document.startLink())
    #expect(document.linking?.slot == Slot(from: "yes", edge: "next"))
    #expect(document.linking?.targets.contains("if") == false, "A link back to the check would loop")
    #expect(document.linking?.target == "no", "Starts at the nearest block it may lead to")
    document.moveLinkTarget(.down)
    #expect(document.linking?.target == "end")
    #expect(!document.finishLink(at: "open"), "Open leads to the check, so this would loop")
    #expect(document.linking != nil)

    undo.beginUndoGrouping()
    #expect(document.finishLink())
    undo.endUndoGrouping()
    #expect(document.linking == nil)
    #expect(document.block("yes")?.kind == .type(text: "y", into: nil, next: "end"))
    undo.undo()
    #expect(document.block("yes")?.kind == .type(text: "y", into: nil, next: nil))

    document.selection = "if"
    #expect(document.startLink())
    #expect(document.linking?.slot.edge == "then")
    #expect(document.startLink(), "Again: the next edge of the same block")
    #expect(document.linking?.slot.edge == "else")
    document.selection = "open"
    #expect(document.linking == nil, "Selecting another block cancels the link")
  }

  @Test func deletesSelectedLinksAsOneStep() throws {
    let document = try document()
    let undo = UndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    let links: Set = [Slot(from: "yes", edge: "next"), Slot(from: "no", edge: "next")]
    document.pickLinks(links)
    #expect(document.pickedLinks == links)
    #expect(document.selection == nil)

    undo.beginUndoGrouping()
    document.unlink(Array(links))
    undo.endUndoGrouping()
    #expect(document.pickedLinks.isEmpty)
    #expect(!document.layout.edges.contains { $0.to == "end" })
    undo.undo()
    #expect(document.layout.edges.filter { $0.to == "end" }.count == 2)
    #expect(!undo.canUndo)
  }

  @Test func zoomFitsAndKeepsThePointUnderThePointer() {
    #expect(Zoom.clamp(5) == Zoom.range.upperBound)
    #expect(Zoom.clamp(0.01) == Zoom.range.lowerBound)
    #expect(Zoom.fit(CGSize(width: 200, height: 100), in: CGSize(width: 800, height: 600)) == 1, "Never blown up")
    #expect(Zoom.fit(CGSize(width: 1000, height: 300), in: CGSize(width: 548, height: 600)) == 0.5)
    #expect(Zoom.fit(CGSize(width: 100_000, height: 100), in: CGSize(width: 800, height: 600)) == Zoom.range.lowerBound)
    #expect(Zoom.fit(.zero, in: CGSize(width: 800, height: 600)) == 1)

    let viewport = CGSize(width: 400, height: 300)
    let zoomed = Zoom.offset(
      CGPoint(x: 100, y: 50), keeping: CGPoint(x: 200, y: 150), from: 1, to: 2,
      content: CGSize(width: 2000, height: 2000), viewport: viewport)
    #expect(zoomed == CGPoint(x: 400, y: 250), "Canvas point (300, 200) stays at (200, 150)")
    let edge = Zoom.offset(
      CGPoint(x: 10, y: 0), keeping: CGPoint(x: 0, y: 0), from: 2, to: 1,
      content: CGSize(width: 400, height: 300), viewport: viewport)
    #expect(edge == .zero, "Never scrolls past the canvas")
  }

  @Test func diffsNotesAndFramesOfTheCanvas() throws {
    var before = try loadFlow(json: Self.branches)
    before.canvas.notes = [
      Note(id: "n1", text: "Old", author: "Ann", time: "", rect: Rect(x: 0, y: 0, width: 200, height: 100))
    ]
    var after = before
    after.canvas.notes[0].text = "New"
    after.canvas.sections = [CanvasSection(id: "s1", title: "Sign in", rect: Rect(x: 0, y: 0, width: 300, height: 200))]
    let diff = diffFlows(before: before, after: after)
    #expect(diff.notes == [BlockChange(id: "n1", kind: .changed, fields: ["text"])])
    #expect(diff.sections == [BlockChange(id: "s1", kind: .added, fields: [])])
    #expect(diff.testFields.isEmpty)
    #expect(FlowDiffView.marks(diff.sections, on: .before).isEmpty, "An added frame isn’t on the old version")
    #expect(FlowDiffView.marks(diff.notes, on: .before) == ["n1": .changed])
    for field in ["text", "title", "author", "position", "size"] {
      #expect(FlowDiffView.fieldName(field) != field)
    }
  }
}

/// Counts the changes registered for undo: each one names its action.
private final class CountingUndoManager: UndoManager {
  var registrations = 0

  override func setActionName(_ actionName: String) {
    registrations += 1
    super.setActionName(actionName)
  }
}
