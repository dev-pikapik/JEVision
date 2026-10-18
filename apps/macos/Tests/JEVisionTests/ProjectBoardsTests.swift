import Foundation
import JEVisionCore
import Testing

@testable import JEVision

@MainActor
struct ProjectBoardsTests {
  private func project() throws -> (ProjectBoards, URL, UserDefaults) {
    let folder = FileManager.default.temporaryDirectory.appending(path: "boards-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let suite = "jevision-tests-\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    let boards = ProjectBoards(defaults: defaults)
    boards.open(folder)
    return (boards, folder, defaults)
  }

  private func tests(on board: Board?) -> [String] {
    (board?.nodes ?? []).compactMap { node in
      if case .file(let file) = node.kind { file } else { nil }
    }
  }

  @Test func namesBoardsAsFilesCanBeCalled() {
    #expect(ProjectBoards.fileName("  Sprint: 12/13 ") == "Sprint- 12-13")
    #expect(ProjectBoards.fileName("..") == "Untitled Board")
    #expect(ProjectBoards.path(of: "Plan") == "boards/Plan.canvas")
    #expect(ProjectBoards.name(of: "boards/Plan.canvas") == "Plan")
    #expect(ProjectBoards.name(of: "boards/old/Plan.canvas") == nil)
    #expect(ProjectBoards.name(of: "Login.flow.json") == nil)
  }

  @Test func makesTheBoardOfAllTestsOnceThereAreTests() throws {
    let (boards, folder, _) = try project()
    boards.sync(tests: [], rootTitle: "Shop")
    #expect(boards.names.isEmpty)

    boards.sync(tests: ["Login.flow.json", "Cart/Pay.flow.json"], rootTitle: "Shop")
    #expect(boards.names == ["All Tests"])
    #expect(boards.allTests == "All Tests")
    #expect(boards.current == "All Tests")
    #expect(Set(tests(on: boards.board)) == ["Login.flow.json", "Cart/Pay.flow.json"])
    #expect(FileManager.default.fileExists(atPath: folder.appending(path: "boards/All Tests.canvas").path))

    boards.sync(tests: ["Login.flow.json"], rootTitle: "Shop")
    #expect(tests(on: boards.board) == ["Login.flow.json"])
    try boards.moveToTrash("All Tests")
    #expect(boards.names == ["All Tests"])
  }

  @Test func createsRenamesAndRemovesBoardsAndTheirLinks() throws {
    let (boards, folder, defaults) = try project()
    let plan = try boards.create("Plan")
    let other = try boards.create("Plan")
    #expect([plan, other] == ["Plan", "Plan 2"])
    #expect(boards.current == "Plan 2")

    let edit = addToBoard(board: try #require(boards.board), kind: .file(file: ProjectBoards.path(of: "Plan")), at: Point(x: 0, y: 0))
    try boards.edit(edit.board, action: "Add", undoManager: nil)
    try boards.rename("Plan", to: "Release: 1")
    #expect(boards.names == ["Plan 2", "Release- 1"])
    #expect(tests(on: boards.board) == ["boards/Release- 1.canvas"])

    let again = ProjectBoards(defaults: defaults)
    again.open(folder)
    #expect(again.current == "Plan 2")

    try boards.moveToTrash("Plan 2")
    #expect(boards.names == ["Release- 1"])
    #expect(boards.current == "Release- 1")
  }

  @Test func undoesAndRedoesEdits() throws {
    let (boards, _, _) = try project()
    try boards.create("Plan")
    let undo = UndoManager()
    undo.groupsByEvent = false
    undo.beginUndoGrouping()
    let edit = addToBoard(board: try #require(boards.board), kind: .text(text: "# Hello"), at: Point(x: 0, y: 0))
    try boards.edit(edit.board, action: "Add Note", undoManager: undo)
    undo.endUndoGrouping()
    #expect(boards.board?.nodes.count == 1)
    #expect(boards.layout.nodes.count == 1)
    #expect(undo.undoActionName == "Add Note")

    undo.undo()
    #expect(boards.board?.nodes.isEmpty == true)
    undo.redo()
    #expect(boards.board?.nodes.first?.kind == .text(text: "# Hello"))
  }

  @Test func acceptsOnlyWebAddresses() {
    #expect(BoardView.webAddress("example.com/docs") == "https://example.com/docs")
    #expect(BoardView.webAddress(" http://127.0.0.1:8080 ") == "http://127.0.0.1:8080")
    #expect(BoardView.webAddress("file:///etc/passwd") == nil)
    #expect(BoardView.webAddress("two words") == nil)
    #expect(BoardView.webAddress("") == nil)
  }
}
