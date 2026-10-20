import Foundation
import JEVisionCore
import Testing

@testable import JEVision

@MainActor
struct LibraryTests {
  private static let test = """
    {"format": 1, "name": "T", "app": "com.apple.TextEdit", "start": "a", "blocks": {
      "a": {"type": "pressKeys", "keys": "a", "next": "b"},
      "b": {"type": "pressKeys", "keys": "b"}}}
    """
  private static let block = """
    {"format": 1, "name": "Sign in as", "icon": "person.badge.key", "app": "com.apple.TextEdit", "start": "email",
     "blocks": {"email": {"type": "type", "text": "${input.email}"}}}
    """

  private func project() throws -> URL {
    let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: folder.appending(path: "blocks"), withIntermediateDirectories: true)
    try Self.test.write(to: folder.appending(path: "t.flow.json"), atomically: true, encoding: .utf8)
    try Self.block.write(to: folder.appending(path: "blocks/sign-in.block.json"), atomically: true, encoding: .utf8)
    try "not a test".write(to: folder.appending(path: "blocks/broken.block.json"), atomically: true, encoding: .utf8)
    return folder
  }

  @Test func readsBuiltInTemplatesThenTheProjectsOwn() throws {
    let folder = try project()
    let library = ProjectLibrary()
    #expect(library.entries.map(\.id) == ["builtin/login", "builtin/onboarding", "builtin/payment"])
    #expect(library.templates.allSatisfy { $0.text.map { (try? loadFlow(json: $0)) != nil } == true })

    library.open(folder)
    let block = try #require(library.blocks.first)
    #expect(library.blocks.count == 1, "A file that isn't a test is left out")
    #expect(block.name == "Sign in as")
    #expect(block.symbol == "person.badge.key")
    #expect(block.inputs == ["email"])
    #expect(block.text == nil)

    let url = try library.save(Self.test, name: "Log in / out", as: .template)
    #expect(url.lastPathComponent == "Log in - out.template.json")
    #expect(url.deletingLastPathComponent().lastPathComponent == "templates")
    let again = try library.save(Self.test, name: "Log in / out", as: .template)
    #expect(again.lastPathComponent == "Log in - out 2.template.json")
    #expect(library.templates.filter { $0.url != nil }.map(\.name) == ["T", "T"])
  }

  @Test func addsTemplatesAndCustomBlocksInOneStep() throws {
    let folder = try project()
    let library = ProjectLibrary()
    library.open(folder)
    let document = FlowDocument(url: folder.appending(path: "t.flow.json"))
    let undo = UndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo

    let login = try #require(library.templates.first)
    undo.beginUndoGrouping()
    document.add(login, at: Slot(from: "a", edge: "next"))
    undo.endUndoGrouping()
    #expect(document.order.first == "a")
    #expect(document.order.last == "b", "The template runs between a and what came after it")
    #expect(document.order.count == 7)
    #expect(document.picked.count == 5, "What was added is selected")
    undo.undo()
    #expect(document.order == ["a", "b"])

    undo.beginUndoGrouping()
    document.add(try #require(library.blocks.first), at: Slot(from: "b", edge: "next"))
    undo.endUndoGrouping()
    let added = try #require(document.selection.flatMap(document.block))
    #expect(added.kind == .runTest(test: "blocks/sign-in.block.json", package: nil, inputs: [:], next: nil))
    #expect(document.issues(of: added.id).contains { $0.code == "flow.input.missing" })
  }

  @Test func paletteListsTheLibraryAfterTheBlocks() {
    let library = ProjectLibrary().entries
    let all = PaletteChoice.all(with: library)
    #expect(all.count == PaletteChoice.all.count + library.count)
    #expect(PaletteChoice.grouped(all).last?.title == String(localized: "Library"))
    #expect(PaletteChoice.search("payment", in: all).first == .library(library[2]))

    var state = PaletteState()
    state.hasTest = true
    state.library = library
    let actions = CommandPalette.entries(state).map(\.action)
    #expect(actions.contains(.addFromLibrary(library[0])))
    #expect(!actions.contains(.saveToLibrary(.template)), "Saving needs selected blocks")
    state.hasPickedBlocks = true
    #expect(CommandPalette.entries(state).map(\.action).contains(.saveToLibrary(.block)))
  }

  @Test func describesPausesAndCustomBlocks() {
    let summary = { (kind: BlockKind) in BlockSummary.text(kind, appName: { $0 }) }
    #expect(summary(.pause(durationMs: 2000, next: nil)) == BlockSummary.duration(2000))
    #expect(summary(.runTest(test: "blocks/sign-in.block.json", package: nil, inputs: [:], next: nil)) == "Runs “sign-in”")
    #expect(
      summary(.runTest(test: "blocks/sign-in.block.json", package: nil, inputs: ["pass": "${secret.P}", "email": "me@x.io"], next: nil))
        == "email: “me@x.io” · pass: secret P")
    #expect(ProjectScanner.testName("sign-in.block.json") == "sign-in")
    #expect(ProjectScanner.testName("Login.flow.json") == "Login")
  }

  @Test func nestsTheStepsOfTestsAndBlocksThatRunInside() {
    let tests = ["t", "t", "login", "login", "card", "t", "pay", "t"]
    #expect(RunReportView.depths(tests, in: "t") == [0, 0, 1, 1, 2, 0, 1, 0])
  }
}
