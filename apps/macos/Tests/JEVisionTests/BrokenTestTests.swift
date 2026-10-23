import Foundation
import JEVisionCore
import Testing

@testable import JEVision

/// A project folder kept by a new git repository whose commits are signed here and never with a key.
func gitProject() throws -> URL {
  let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
  _ = try gitStartTracking(project: root.path(percentEncoded: false))
  let config = root.appending(path: ".git/config")
  let local = "[user]\n\tname = Anna\n\temail = anna@example.com\n[commit]\n\tgpgsign = false\n"
  try (String(contentsOf: config, encoding: .utf8) + local).write(to: config, atomically: true, encoding: .utf8)
  return root
}

@MainActor
struct BrokenTestTests {
  private static let path = "login/sign-in.flow.json"
  private static let test = #"{"format": 1, "name": "Sign in", "app": "com.apple.TextEdit", "start": "a", "blocks": {"a": {"type": "pressKeys", "keys": "a"}}}"#

  private static func write(_ text: String, in root: URL) throws {
    let file = root.appending(path: path)
    try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
    try text.write(to: file, atomically: true, encoding: .utf8)
  }

  private static func commit(_ text: String, _ message: String, in root: URL) throws -> String {
    try write(text, in: root)
    return try gitCommit(project: root.path(percentEncoded: false), paths: [path], message: message)
  }

  @Test func aBrokenTestComesBackAsTheLastCommittedVersionThatOpens() throws {
    let root = try gitProject()
    defer { try? FileManager.default.removeItem(at: root) }
    let good = try Self.commit(Self.test, "Add sign-in", in: root)
    _ = try Self.commit(#"{"format": 1, "name": "#, "Half a test", in: root)
    try Self.write("not a test", in: root)
    let document = FlowDocument(url: root.appending(path: Self.path), project: root)
    #expect(document.flow == nil)
    #expect(document.failure != nil)

    let recovery = try TestRecovery.find(Self.path, in: root)
    guard case .version(let commit, let source) = recovery else {
      Issue.record("no version to restore: \(recovery)")
      return
    }
    #expect(commit.id == good, "the newest commit holds a broken file, so it is the one before")
    #expect(source == Self.path)

    let file = root.appending(path: Self.path)
    let undo = UndoManager()
    undo.groupsByEvent = false
    undo.beginUndoGrouping()
    // The way the view restores, with Git alone: the view also moves the broken file to the Trash,
    // and a test leaves the Trash alone.
    try TestRecovery.restore(file, open: { [document] }, in: undo, ask: { _, _ in false }) {
      try gitRestore(project: root.path(percentEncoded: false), commit: commit.id, source: source, path: Self.path)
    }
    undo.endUndoGrouping()
    let restored = try Data(contentsOf: file)
    #expect(document.failure == nil)
    #expect(document.flow?.name == "Sign in")
    #expect(undo.undoActionName == "Restore Version")

    undo.undo()
    #expect(try String(contentsOf: file, encoding: .utf8) == "not a test")
    #expect(document.flow == nil, "Undo shows the broken file again, so the next edit can't save over it")
    #expect(document.failure != nil)

    undo.redo()
    #expect(try Data(contentsOf: file) == restored)
    #expect(document.flow?.name == "Sign in", "Redo shows the restored test")
  }

  @Test func undoOfARestoreFromTheHistoryShowsInTheOpenTest() throws {
    let root = try gitProject()
    defer { try? FileManager.default.removeItem(at: root) }
    let first = try Self.commit(Self.test, "Add sign-in", in: root)
    _ = try Self.commit(Self.test.replacing("Sign in", with: "Log in"), "Rename", in: root)
    let file = root.appending(path: Self.path)
    let current = try Data(contentsOf: file)
    var open: [FlowDocument] = []
    let undo = UndoManager()
    undo.groupsByEvent = false
    undo.beginUndoGrouping()
    try TestRecovery.restore(file, open: { open }, in: undo, ask: { _, _ in false }) {
      try gitRestore(project: root.path(percentEncoded: false), commit: first, source: Self.path, path: Self.path)
    }
    undo.endUndoGrouping()
    // The history opens the test it put back once it closes.
    let document = FlowDocument(url: file, project: root)
    open = [document]
    #expect(document.flow?.name == "Sign in")

    undo.undo()
    #expect(try Data(contentsOf: file) == current)
    #expect(document.flow?.name == "Log in", "the open test shows what Undo put back, so its next edit can't save over it")
    undo.redo()
    #expect(document.flow?.name == "Sign in")
  }

  @Test func withoutAVersionThatOpensItSaysWhy() throws {
    let plain = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: plain) }
    try Self.write("not a test", in: plain)
    #expect(try TestRecovery.find(Self.path, in: plain) == .noGit)

    let root = try gitProject()
    defer { try? FileManager.default.removeItem(at: root) }
    try Self.write("not a test", in: root)
    let never = try TestRecovery.find(Self.path, in: root)
    #expect(never == .notCommitted)
    #expect(!never.isCommitted)

    _ = try Self.commit("not a test", "Broken from the start", in: root)
    let broken = try TestRecovery.find(Self.path, in: root)
    #expect(broken == .nothingOpens)
    #expect(broken.isCommitted, "its history can still be looked through")
  }
}
