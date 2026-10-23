import Foundation
import JEVisionCore
import Testing

@testable import JEVision

@MainActor
struct UnusedNoteFilesTests {
  /// Also with the project opened as the file system names it, `/private/var/…` rather than the
  /// link `/var/…`: Foundation drops `/private` from a path that exists, not from a file that's gone.
  @Test(arguments: [false, true]) func unusedFilesGoToTheTrashAsOneStepThatUndoes(realPath: Bool) throws {
    let made = try gitProject()
    defer { try? FileManager.default.removeItem(at: made) }
    let real = try #require(realpath(made.path, nil))
    defer { free(real) }
    let root = realPath ? URL(filePath: String(cString: real), directoryHint: .isDirectory) : made
    if realPath { #expect(root.standardizedFileURL.path != root.path, "the folder is reached through a link") }
    let url = root.appending(path: "login/sign-in.flow.json")
    try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
    try #"{"format": 1, "name": "Sign in", "app": "com.apple.TextEdit", "start": "a", "blocks": {"a": {"type": "pressKeys", "keys": "a"}}}"#
      .write(to: url, atomically: true, encoding: .utf8)
    let document = FlowDocument(url: url, project: root)
    document.addNote(at: Point(x: 0, y: 0), author: "Anna")
    let note = try #require(document.flow?.canvas.notes.first?.id)
    document.attach(Attachment(kind: .file, value: "kept.png"), to: note)
    #expect(document.unusedNoteFiles == nil, "no notes folder yet")

    let notes = root.appending(path: "login/sign-in.assets/notes")
    let kept = notes.appending(path: "\(note)/kept.png")
    let detached = notes.appending(path: "\(note)/old.png")
    let gone = notes.appending(path: "gone")
    let deleted = gone.appending(path: "clip.mov")
    for (file, size) in [(kept, 10), (detached, 100), (deleted, 1000)] {
      try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
      try Data(count: size).write(to: file)
    }
    let attributes = root.appending(path: ".gitattributes")
    let lfs = " filter=lfs diff=lfs merge=lfs -text\n"
    let tracked =
      "*.psd" + lfs + "login/sign-in.assets/notes/\(note)/kept.png" + lfs
      + "login/sign-in.assets/notes/\(note)/old.png" + lfs + "login/sign-in.assets/notes/gone/clip.mov" + lfs
    try tracked.write(to: attributes, atomically: true, encoding: .utf8)
    let left = "*.psd" + lfs + "login/sign-in.assets/notes/\(note)/kept.png" + lfs

    let unused = try #require(document.unusedNoteFiles)
    #expect(unused.paths == [gone, detached].map { $0.path(percentEncoded: false) })
    #expect(unused.size == 1100)

    let trash = root.appending(path: "Trash")
    try FileManager.default.createDirectory(at: trash, withIntermediateDirectories: true)
    let toTrash: Baseline.Trash = { file in
      let trashed = trash.appending(path: UUID().uuidString)
      try FileManager.default.moveItem(at: file, to: trashed)
      return trashed
    }
    let undo = UndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    var shown = 0
    try document.removeUnusedNoteFiles(unused, project: root, trash: toTrash, ask: { _, _ in false }) { shown += 1 }

    let exists = { (file: URL) in FileManager.default.fileExists(atPath: file.path(percentEncoded: false)) }
    #expect(!exists(detached) && !exists(gone))
    #expect(exists(kept))
    #expect(try String(contentsOf: attributes, encoding: .utf8) == left, "Git LFS lets go of the files that went")
    #expect(document.unusedNoteFiles == nil)
    #expect(undo.undoActionName == "Remove Unused Note Files")

    // The test changed somewhere else and read again: the steps that edited it go, this one stays whole.
    let renamed = try String(contentsOf: url, encoding: .utf8).replacing(#""Sign in""#, with: #""Log in""#)
    try renamed.write(to: url, atomically: true, encoding: .utf8)
    document.reload()
    #expect(document.flow?.name == "Log in")

    undo.undo()
    #expect(exists(detached) && exists(deleted) && exists(kept))
    #expect(try String(contentsOf: attributes, encoding: .utf8) == tracked)
    #expect(document.unusedNoteFiles?.paths == unused.paths)
    #expect(shown == 1)
    #expect(!undo.canUndo, "one step")

    undo.redo()
    #expect(!exists(detached) && !exists(deleted))
    #expect(try String(contentsOf: attributes, encoding: .utf8) == left)
    #expect(shown == 2)
  }

  /// Also when a file doesn't go to the Trash: the alert tells that and the lines left behind.
  @Test(arguments: [false, true]) func linesThatCantBeDroppedAreReported(aFileStays: Bool) throws {
    let root = try gitProject()
    let folder = root.path(percentEncoded: false)
    defer {
      try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: folder)
      try? FileManager.default.removeItem(at: root)
    }
    let gone = root.appending(path: "sign-in.assets/notes/gone")
    try FileManager.default.createDirectory(at: gone, withIntermediateDirectories: true)
    try Data(count: 5).write(to: gone.appending(path: "clip.mov"))
    let attributes = root.appending(path: ".gitattributes")
    let tracked = "sign-in.assets/notes/gone/clip.mov filter=lfs diff=lfs merge=lfs -text\n"
    try tracked.write(to: attributes, atomically: true, encoding: .utf8)
    let stuck = root.appending(path: "sign-in.assets/notes/old.png")
    try Data(count: 3).write(to: stuck)
    struct Locked: LocalizedError {
      var errorDescription: String? { "“old.png” is locked." }
    }
    let toTrash: Baseline.Trash = { file in
      if file.lastPathComponent == stuck.lastPathComponent { throw Locked() }
      let trashed = root.appending(path: "sign-in.assets/\(UUID().uuidString)")
      try FileManager.default.moveItem(at: file, to: trashed)
      return trashed
    }
    // Nothing new can be written in the project's folder, so .gitattributes can't be replaced.
    try FileManager.default.setAttributes([.posixPermissions: 0o555], ofItemAtPath: folder)
    let document = FlowDocument(url: root.appending(path: "sign-in.flow.json"), project: root)
    let unused = UnusedNoteFiles(paths: ([gone] + (aFileStays ? [stuck] : [])).map { $0.path(percentEncoded: false) }, size: 8)

    let failure = #expect(throws: FlowDocument.GitAttributesFailure.self) {
      try document.removeUnusedNoteFiles(unused, project: root, trash: toTrash, ask: { _, _ in false })
    }
    #expect(
      failure?.errorDescription
        == (aFileStays ? "“old.png” is locked." : "The files are in the Trash, but .gitattributes still lists them"))
    #expect(failure?.recoverySuggestion?.contains("Permission denied") == true, "why the lines stayed")
    #expect(!FileManager.default.fileExists(atPath: gone.path(percentEncoded: false)))
    #expect(FileManager.default.fileExists(atPath: stuck.path(percentEncoded: false)))
    #expect(try String(contentsOf: attributes, encoding: .utf8) == tracked)
  }

  @Test func aTestThatCantBeReadOffersNothing() throws {
    let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: folder) }
    let stray = folder.appending(path: "sign-in.assets/notes/gone/clip.mov")
    try FileManager.default.createDirectory(at: stray.deletingLastPathComponent(), withIntermediateDirectories: true)
    try Data(count: 5).write(to: stray)
    let url = folder.appending(path: "sign-in.flow.json")
    try "not a test".write(to: url, atomically: true, encoding: .utf8)
    #expect(FlowDocument(url: url).unusedNoteFiles == nil)
  }
}
