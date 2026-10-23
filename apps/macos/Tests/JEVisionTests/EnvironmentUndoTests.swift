import Foundation
import Testing

@testable import JEVision

@MainActor
struct EnvironmentUndoTests {
  private static let staging = #"{"format":1,"environments":{"staging":{"HOST":"s"},"production":{"HOST":"p"}}}"#

  /// A project folder with `environments` as its environments file, when given; the environments
  /// opened from it, with their own store for the choice; and an undo manager grouping as told.
  @MainActor
  private final class Fixture {
    let project = FileManager.default.temporaryDirectory.appending(path: "jevision-env-undo-\(UUID().uuidString)")
    let suite = "jevision-tests-\(UUID().uuidString)"
    let environments: ProjectEnvironments
    let undo = UndoManager()

    init(_ environments: String?) throws {
      try FileManager.default.createDirectory(at: project, withIntermediateDirectories: true)
      if let environments {
        try environments.write(to: ProjectEnvironments.file(in: project), atomically: true, encoding: .utf8)
      }
      self.environments = ProjectEnvironments(defaults: try #require(UserDefaults(suiteName: suite)))
      self.environments.open(project)
      undo.groupsByEvent = false
    }

    var file: URL { ProjectEnvironments.file(in: project) }

    func value(_ name: String, in environment: String) -> String? {
      environments.current.all.first { $0.name == environment }?.values[name]
    }

    func step(_ change: () throws -> Void) rethrows {
      undo.beginUndoGrouping()
      defer { undo.endUndoGrouping() }
      try change()
    }

    deinit {
      try? FileManager.default.removeItem(at: project)
      UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite)
    }
  }

  @Test func aChangedValueIsUndoneAndRedone() throws {
    let fixture = try Fixture(Self.staging)
    let before = try Data(contentsOf: fixture.file)

    try fixture.step {
      try fixture.environments.setValue(
        "https://s.example.com", named: "HOST", in: "staging", undoManager: fixture.undo, ask: { _, _ in false })
    }
    let after = try Data(contentsOf: fixture.file)
    #expect(after != before)
    #expect(fixture.undo.undoActionName == "Change Value")

    fixture.undo.undo()
    #expect(try Data(contentsOf: fixture.file) == before)
    #expect(fixture.value("HOST", in: "staging") == "s", "the panel shows the file at once")
    fixture.undo.redo()
    #expect(try Data(contentsOf: fixture.file) == after)
    #expect(fixture.value("HOST", in: "staging") == "https://s.example.com")
  }

  @Test func anAddedEnvironmentIsUndone() throws {
    let fixture = try Fixture(nil)

    try fixture.step { try fixture.environments.add("staging", values: ["HOST": "s"], undoManager: fixture.undo) }
    #expect(fixture.environments.current.all.map(\.name) == ["staging"])
    #expect(fixture.undo.undoActionName == "Add Environment")

    fixture.undo.undo()
    #expect(fixture.environments.current.all.isEmpty)
    #expect(!FileManager.default.fileExists(atPath: fixture.file.path(percentEncoded: false)))
    fixture.undo.redo()
    #expect(fixture.environments.current.all.map(\.name) == ["staging"])
  }

  @Test(arguments: [false, true])
  func undoLeavesAFileChangedOutsideUnlessThePersonAgrees(agrees: Bool) throws {
    let fixture = try Fixture(Self.staging)
    let before = try Data(contentsOf: fixture.file)
    var asked: [String] = []
    try fixture.step {
      try fixture.environments.setValue("x", named: "HOST", in: "staging", undoManager: fixture.undo) { _, action in
        asked.append(action)
        return agrees
      }
    }
    let outside = Data(#"{"format":1,"environments":{"staging":{"HOST":"from git"}}}"#.utf8)
    try outside.write(to: fixture.file)
    fixture.environments.reload()

    fixture.undo.undo()
    #expect(asked == ["Change Value"])
    #expect(try Data(contentsOf: fixture.file) == (agrees ? before : outside))
    #expect(fixture.value("HOST", in: "staging") == (agrees ? "s" : "from git"))
  }

  @Test func choosingAnEnvironmentIsUndone() throws {
    let fixture = try Fixture(Self.staging)
    #expect(fixture.environments.current.selected?.name == "production")

    fixture.step { fixture.environments.select("staging", undoManager: fixture.undo) }
    #expect(fixture.environments.current.selected?.name == "staging")
    #expect(fixture.undo.undoActionName == "Choose Environment")
    let again = UndoManager()
    fixture.environments.select("staging", undoManager: again)
    #expect(!again.canUndo, "choosing the one already chosen isn't a step")

    fixture.undo.undo()
    #expect(fixture.environments.current.selected?.name == "production")
    fixture.undo.redo()
    #expect(fixture.environments.current.selected?.name == "staging")
  }

  @Test func aValueTypedWhileTheFileChangedIsLeftToThePerson() {
    #expect(ValueDraft(typed: "a", base: "a", file: "a") == .unchanged)
    #expect(ValueDraft(typed: "a", base: "a", file: "b") == .unchanged, "nothing typed: the file's value stays")
    #expect(ValueDraft(typed: "b", base: "a", file: "b") == .unchanged, "the file holds it already")
    #expect(ValueDraft(typed: "b", base: "a", file: "a") == .save)
    #expect(ValueDraft(typed: "b", base: nil, file: nil) == .save)
    #expect(ValueDraft(typed: "b", base: "a", file: "c") == .conflict)
    #expect(ValueDraft(typed: "b", base: nil, file: "c") == .conflict, "set elsewhere while the sheet was open")
    #expect(ValueDraft(typed: "b", base: "a", file: nil) == .conflict, "removed from the file meanwhile")
  }

  @Test func stepsLetTheirUndoManagerGo() throws {
    let fixture = try Fixture(Self.staging)
    weak var released: UndoManager?
    try autoreleasepool {
      let undo = UndoManager()
      undo.groupsByEvent = false
      undo.beginUndoGrouping()
      fixture.environments.select("staging", undoManager: undo)
      try fixture.environments.setValue("q", named: "HOST", in: "staging", undoManager: undo, ask: { _, _ in false })
      undo.endUndoGrouping()
      #expect(undo.canUndo)
      released = undo
    }
    #expect(released == nil, "a sheet's undo manager goes away with the sheet")
  }
}
