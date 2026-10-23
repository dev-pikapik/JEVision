import AppKit
import JEVisionCore
import SwiftUI
import Testing

@testable import JEVision

@MainActor
struct DataTableUndoTests {
  private func folder() throws -> URL {
    let made = FileManager.default.temporaryDirectory.appending(path: "jevision-data-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: made, withIntermediateDirectories: true)
    return made
  }

  private func undoManager() -> UndoManager {
    let undo = UndoManager()
    undo.groupsByEvent = false
    return undo
  }

  /// Runs `change` as one event, the way a click or a keystroke does.
  private func step(_ undo: UndoManager, _ change: () throws -> Void) rethrows {
    undo.beginUndoGrouping()
    defer { undo.endUndoGrouping() }
    try change()
  }

  /// ⌘Z: undoes the last step that changed the table. Grouping each change by hand, as these
  /// tests do, leaves an empty group for each keystroke that joined the step before it; the app
  /// groups by event and opens no group for those.
  private func undoStep(_ undo: UndoManager, _ document: DataTableDocument) {
    let before = document.table
    while undo.canUndo, document.table == before { undo.undo() }
  }

  /// ⇧⌘Z, the same way.
  private func redoStep(_ undo: UndoManager, _ document: DataTableDocument) {
    let before = document.table
    while undo.canRedo, document.table == before { undo.redo() }
  }

  private let never: FileUndo.Ask = { _, _ in
    Issue.record("asked to replace the file")
    return false
  }

  @Test func undoesAndRedoesEveryChange() throws {
    let url = try folder().appending(path: "people.csv")
    try writeDataRows(path: url.path, table: DataRows(columns: ["name"], rows: [["Ann"]]))
    let document = DataTableDocument(url: url)
    let undo = undoManager()
    document.undoManager = undo
    let start = document.table

    step(undo) { document.addRow() }
    #expect(undo.undoActionName == "Add Row")
    step(undo) { document.insertColumn(at: 0) }
    #expect(undo.undoActionName == "Insert Column")
    // Typing into one cell, or one column name, undoes as one step.
    for name in ["a", "ag", "age"] { step(undo) { document.rename(column: 0, to: name) } }
    for text in ["B", "Bo", "Bob"] { step(undo) { document.set(text, row: 1, column: 1) } }
    #expect(document.table == DataRows(columns: ["age", "name"], rows: [["", "Ann"], ["", "Bob"]]))
    #expect(document.hasUnsavedChanges)

    undoStep(undo, document)
    #expect(document.table.rows == [["", "Ann"], ["", ""]])
    #expect(undo.redoActionName == "Edit Cell")
    undoStep(undo, document)
    #expect(document.table.columns == ["column2", "name"])
    #expect(undo.redoActionName == "Rename Column")
    undoStep(undo, document)
    undoStep(undo, document)
    #expect(document.table == start)
    #expect(!document.hasUnsavedChanges)
    #expect(!undo.canUndo)

    redoStep(undo, document)
    redoStep(undo, document)
    redoStep(undo, document)
    #expect(document.table.columns == ["age", "name"])

    step(undo) { document.deleteRow(0) }
    #expect(undo.undoActionName == "Delete Row")
    step(undo) { document.deleteColumn(1) }
    #expect(undo.undoActionName == "Delete Column")
    #expect(document.table == DataRows(columns: ["age"], rows: [[""]]))
    undoStep(undo, document)
    undoStep(undo, document)
    #expect(document.table == DataRows(columns: ["age", "name"], rows: [["", "Ann"], ["", ""]]))
  }

  @Test func savingIsOneStepOfTheWindowThatPutsTheFileBack() throws {
    let url = try folder().appending(path: "people.csv")
    try writeDataRows(path: url.path, table: DataRows(columns: ["name"], rows: [["Ann"]]))
    let before = try Data(contentsOf: url)
    let document = DataTableDocument(url: url)
    document.set("Bob", row: 0, column: 0)
    let window = undoManager()
    var shown = 0

    try step(window) { try document.save(undoManager: window, ask: never) { shown += 1 } }
    #expect(window.undoActionName == "Edit Data Table")
    #expect(try readDataRows(path: url.path).rows == [["Bob"]])
    #expect(!document.hasUnsavedChanges)
    #expect(document.outsideChange() == nil)

    window.undo()
    #expect(try Data(contentsOf: url) == before)
    #expect(shown == 1)
    window.redo()
    #expect(try readDataRows(path: url.path).rows == [["Bob"]])
    #expect(shown == 2)
  }

  @Test func undoOfANewTableTakesTheFileAway() throws {
    let url = try folder().appending(path: "new.csv")
    let document = DataTableDocument(url: url)
    let window = undoManager()

    try step(window) { try document.save(undoManager: window, ask: never) {} }
    #expect(window.undoActionName == "Create Data Table")
    #expect(FileManager.default.fileExists(atPath: url.path))
    window.undo()
    #expect(!FileManager.default.fileExists(atPath: url.path))
  }

  @Test func aNewTableAsksBeforeClosingOnceSomethingIsTyped() throws {
    let document = DataTableDocument(url: try folder().appending(path: "new.csv"))
    #expect(!document.hasUnsavedChanges)
    #expect(document.canSave)
    document.set("Ann", row: 0, column: 0)
    #expect(document.hasUnsavedChanges)
  }

  @Test func noticesTheFileChangedSomewhereElse() throws {
    let folder = try folder()
    let url = folder.appending(path: "people.csv")
    try writeDataRows(path: url.path, table: DataRows(columns: ["name"], rows: [["Ann"]]))
    let document = DataTableDocument(url: url)
    #expect(document.outsideChange() == nil)
    try writeDataRows(path: url.path, table: DataRows(columns: ["name"], rows: [["Eve"]]))
    #expect(document.outsideChange() == .changed)
    try FileManager.default.removeItem(at: url)
    #expect(document.outsideChange() == .removed)

    let new = DataTableDocument(url: folder.appending(path: "new.csv"))
    #expect(new.outsideChange() == nil)
    try writeDataRows(path: new.url.path, table: DataRows(columns: ["name"], rows: [["Eve"]]))
    #expect(new.outsideChange() == .created)
  }

  @Test func theFilesVersionIsAStepThatUndoBringsTheEditsBackFrom() throws {
    let url = try folder().appending(path: "people.csv")
    try writeDataRows(path: url.path, table: DataRows(columns: ["name"], rows: [["Ann"]]))
    let document = DataTableDocument(url: url)
    let undo = undoManager()
    document.undoManager = undo
    step(undo) { document.set("Bob", row: 0, column: 0) }
    try writeDataRows(path: url.path, table: DataRows(columns: ["name"], rows: [["Eve"]]))

    try step(undo) { try document.useFileVersion() }
    #expect(document.table.rows == [["Eve"]])
    #expect(document.outsideChange() == nil)
    #expect(!document.hasUnsavedChanges)
    undo.undo()
    #expect(document.table.rows == [["Bob"]])
    #expect(document.hasUnsavedChanges)
  }

  // MARK: Typing in a window

  /// The editor in a window kept off screen, without any input sent to the Mac, and its table.
  private func host(_ table: DataRows) throws -> (NSWindow, DataTableDocument) {
    let folder = try folder()
    try writeDataRows(path: folder.appending(path: "people.csv").path, table: table)
    let context = FieldContext(
      issues: [], folder: folder, projectRoot: nil, document: FlowDocument(url: folder.appending(path: "t.flow.json")),
      block: "data")
    let document = DataTableDocument(url: folder.appending(path: "people.csv"))
    let editor = DataTableEditor(document: document, file: "people.csv", context: context, undoManager: nil)
    let window = NSWindow(
      contentRect: NSRect(x: -10000, y: -10000, width: 900, height: 600), styleMask: [.titled], backing: .buffered,
      defer: false)
    window.isReleasedWhenClosed = false
    window.contentView = NSHostingView(rootView: editor)
    window.orderFront(nil)
    settle()
    #expect(document.undoManager === window.undoManager)
    return (window, document)
  }

  private func settle() { RunLoop.main.run(until: Date().addingTimeInterval(0.2)) }

  /// Clicks into the leftmost cell that shows `text` and types `keys` after it, one event each.
  private func type(_ keys: [String], into text: String = "", in window: NSWindow) throws {
    func fields(_ view: NSView) -> [NSTextField] {
      view.subviews.flatMap { ($0 as? NSTextField).map { [$0] } ?? fields($0) }
    }
    let cell = try #require(
      fields(window.contentView!).filter { $0.stringValue == text }
        .min { $0.convert($0.bounds, to: nil).minX < $1.convert($1.bounds, to: nil).minX })
    window.makeFirstResponder(cell)
    (window.firstResponder as? NSTextView)?.moveToEndOfDocument(nil)
    let typing = try #require(window.firstResponder as? NSTextView)
    for key in keys {
      typing.insertText(key, replacementRange: NSRange(location: NSNotFound, length: 0))
      settle()
    }
  }

  @Test func changingTheTableWhileTypingInACellUndoesStepByStep() throws {
    let (window, document) = try host(DataRows(columns: ["name", "age"], rows: [["Ann", "1"]]))
    defer { window.close() }
    let undo = try #require(window.undoManager)
    document.addRow()
    settle()
    try type(["C", "y"], in: window)
    #expect(document.table.rows == [["Ann", "1"], ["Cy", ""]])

    document.deleteColumn(0)
    settle()
    #expect(!(window.firstResponder is NSTextView), "Typing ends, as Tab ends it")

    // Each ⌘Z undoes one change of the table: the column, then the cell, then the row.
    let steps = [
      ("Delete Column", [["Ann", "1"], ["Cy", ""]]),
      ("Edit Cell", [["Ann", "1"], ["", ""]]),
      ("Add Row", [["Ann", "1"]]),
    ]
    for (name, rows) in steps {
      #expect(undo.undoActionName == name)
      _ = window.firstResponder?.tryToPerform(Selector(("undo:")), with: nil)
      settle()
      #expect(document.table == DataRows(columns: ["name", "age"], rows: rows))
    }
    #expect(!undo.canUndo)
  }

  @Test func savingEndsTypingInACell() throws {
    let (window, document) = try host(DataRows(columns: ["name"], rows: [[""]]))
    defer { window.close() }
    try type(["C", "y"], in: window)

    let parent = undoManager()
    try step(parent) { try document.save(undoManager: parent, ask: never) {} }
    #expect(!(window.firstResponder is NSTextView), "Typing ends, as Tab ends it")
    #expect(try readDataRows(path: document.url.path).rows == [["Cy"]])
  }

  @Test func typingAgainInACellAfterLeavingItIsANewStep() throws {
    let (window, document) = try host(DataRows(columns: ["name"], rows: [[""]]))
    defer { window.close() }
    let undo = try #require(window.undoManager)
    try type(["C", "y"], in: window)
    window.makeFirstResponder(nil)
    settle()
    try type(["d"], into: "Cy", in: window)
    window.makeFirstResponder(nil)
    settle()
    #expect(document.table.rows == [["Cyd"]])

    undo.undo()
    #expect(document.table.rows == [["Cy"]])
    undo.undo()
    #expect(document.table.rows == [[""]])
  }
}
