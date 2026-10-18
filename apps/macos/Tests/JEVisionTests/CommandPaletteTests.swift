import Foundation
import JEVisionCore
import SwiftUI
import Testing

@testable import JEVision

struct CommandPaletteTests {
  private func actions(_ state: PaletteState) -> [PaletteAction] {
    CommandPalette.entries(state).map(\.action)
  }

  private var editing: PaletteState {
    var state = PaletteState()
    state.hasProject = true
    state.hasTest = true
    state.canRun = true
    state.hasTests = true
    state.tests = [
      ("Login/Sign In.flow.json", "Sign In", "Login", ""),
      ("Checkout.flow.json", "Checkout", "", "Pay for the basket 4242 4242"),
    ]
    return state
  }

  @Test func offersOnlyWhatCanBeDoneNow() {
    let closed = actions(PaletteState())
    #expect(closed == [.openProject, .settings])

    let open = actions(editing)
    #expect(open.contains(.run))
    #expect(open.contains(.record))
    #expect(open.contains(.addCondition))
    #expect(!open.contains(.runFromSelection), "Nothing is selected")
    #expect(!open.contains(.disconnect), "Nothing is selected")
    #expect(!open.contains(.openReport), "The test hasn't run")
    #expect(!open.contains(.commit), "The project isn't in Git")
    #expect(open.filter { if case .addBlock = $0 { true } else { false } }.count == BlockInfo.catalog.count)
    #expect(open.suffix(2) == [.openTest("Login/Sign In.flow.json"), .openTest("Checkout.flow.json")])

    var running = editing
    running.canRun = false
    #expect(!actions(running).contains(.run))
    #expect(!actions(running).contains(.record))
  }

  @Test func followsTheSelectionAndGit() {
    var state = editing
    state.selection = "check"
    state.selectionHasLinks = true
    state.git = PaletteState.Git(
      isTracked: true, hasChanges: true, hasCommits: true, hasUpstream: true, hasRemote: true, onGitHub: true)
    let found = actions(state)
    for action in [PaletteAction.runFromSelection, .disconnect, .commit, .newPullRequest, .pull, .push] {
      #expect(found.contains(action))
    }
    #expect(CommandPalette.entries(state).first { $0.action == .addCondition }?.title == "Add If")

    state.selectionIsIf = true
    #expect(CommandPalette.entries(state).first { $0.action == .addCondition }?.title == "Add Condition")

    state.git.isBusy = true
    #expect(!actions(state).contains(.push), "Git is busy with another action")
  }

  @Test func searchFindsCommandsBlocksAndTests() {
    let entries = CommandPalette.entries(editing)
    func first(_ query: String) -> PaletteAction? { CommandPalette.search(query, in: entries).first?.action }
    #expect(first("rec") == .record)
    #expect(first("run") == .run)
    #expect(first("click") == .addBlock("click"))
    #expect(first("sign") == .openTest("Login/Sign In.flow.json"))
    #expect(first("login") == .openTest("Login/Sign In.flow.json"), "A test is found by its folder")
    #expect(first("debug") == .runStepByStep, "A command is found by its keywords")
    #expect(first("basket 4242") == .openTest("Checkout.flow.json"), "A test is found by what it says and does")
    #expect(CommandPalette.search("", in: entries) == entries)
    #expect(CommandPalette.search("zzzz", in: entries).isEmpty)
  }

  @Test func paletteSearchesAndGroupsBlocks() {
    #expect(PaletteChoice.search("wait").first == .block("waitFor"))
    #expect(PaletteChoice.search("note").first == .note)
    #expect(PaletteChoice.search("").count == PaletteChoice.all.count)

    let groups = PaletteChoice.grouped()
    #expect(groups.flatMap(\.choices) == PaletteChoice.all)
    #expect(Set(groups.map(\.title)).count == groups.count, "Each group is listed once")
    #expect(PaletteChoice.all.allSatisfy { !$0.tagline.isEmpty })
  }

  @Test func writesShortcutsTheWayMenusDo() {
    #expect(Shortcuts.symbols(Shortcuts.commandPalette) == "⌘K")
    #expect(Shortcuts.symbols(Shortcuts.record) == "⇧⌘R")
    #expect(Shortcuts.symbols(Shortcuts.runAll) == "⌃⌘R")
    #expect(Shortcuts.symbols(Shortcuts.disconnect) == "⌥⌘L")
    #expect(Shortcuts.symbols(Shortcuts.runFromSelection) == "⇧⌘↩")
  }
}

@MainActor
struct FlowDocumentShortcutTests {
  private static let chain = """
    {"format": 1, "name": "T", "app": "com.apple.TextEdit", "start": "a", "blocks": {
      "a": {"type": "pressKeys", "keys": "a", "next": "b"},
      "b": {"type": "pressKeys", "keys": "b"}}}
    """

  private func document() throws -> (FlowDocument, UndoManager) {
    let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let url = folder.appending(path: "t.flow.json")
    try Self.chain.write(to: url, atomically: true, encoding: .utf8)
    let document = FlowDocument(url: url)
    let undo = UndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    return (document, undo)
  }

  private func conditions(_ document: FlowDocument, _ id: String) -> Int? {
    if case .if(_, _, let elseIf, _) = document.block(id)?.kind { return elseIf.count + 1 }
    return nil
  }

  private func step(_ undo: UndoManager, _ change: () -> Void) {
    undo.beginUndoGrouping()
    change()
    undo.endUndoGrouping()
  }

  @Test func addsAnIfAfterTheSelectionThenGrowsIt() throws {
    let (document, undo) = try document()
    document.selection = "a"
    #expect(document.slotAfterSelection == Slot(from: "a", edge: "next"))

    step(undo) { document.addCondition() }
    let id = try #require(document.selection)
    #expect(conditions(document, id) == 1)
    #expect(document.order == ["a", id, "b"])
    #expect(document.slotAfterSelection == Slot(from: id, edge: "then"))

    step(undo) { document.addCondition() }
    #expect(document.selection == id)
    #expect(conditions(document, id) == 2)
    #expect(undo.undoActionName == "Add Condition")

    undo.undo()
    #expect(conditions(document, id) == 1)
    undo.undo()
    #expect(document.order == ["a", "b"])
  }

  @Test func disconnectsABlockAndUndoes() throws {
    let (document, undo) = try document()
    #expect(document.hasLinks("a"))
    step(undo) { document.disconnect("b") }
    #expect(!document.hasLinks("a"))
    #expect(!document.hasLinks("b"))
    #expect(undo.undoActionName == "Disconnect")
    undo.undo()
    #expect(document.hasLinks("b"))
  }
}
