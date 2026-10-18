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
    #expect(summary(.openFile(files: ["docs/a.txt", "b.txt", "c.txt"], app: nil, newInstance: false, next: nil)) == "a.txt and 2 more")
    #expect(summary(.runTest(test: "shared/sign-in.flow.json", next: nil)) == "Runs “sign-in”")
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
    #expect(KeyCombo.from(keyCode: 1, characters: "s", modifierFlags: [.shift, .command]) == "cmd+shift+s")
    #expect(KeyCombo.from(keyCode: 36, characters: "\r", modifierFlags: []) == "return")
    #expect(KeyCombo.from(keyCode: 96, characters: nil, modifierFlags: [.control]) == "ctrl+f5")
    #expect(KeyCombo.from(keyCode: 0, characters: "é", modifierFlags: []) == nil)
  }
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

/// Counts the changes registered for undo: each one names its action.
private final class CountingUndoManager: UndoManager {
  var registrations = 0

  override func setActionName(_ actionName: String) {
    registrations += 1
    super.setActionName(actionName)
  }
}
