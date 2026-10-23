import Foundation
import JEVisionCore
import Testing

@testable import JEVision

@MainActor
struct FileUndoTests {
  @Test func aFailedRelatedFileSaveRestoresBothFiles() throws {
    enum Failed: Error { case save }
    let files = FileManager.default
    let folder = files.temporaryDirectory.appending(path: "jevision-file-undo-\(UUID().uuidString)")
    try files.createDirectory(at: folder, withIntermediateDirectories: true)
    defer { try? files.removeItem(at: folder) }
    let csv = folder.appending(path: "table.csv")
    let meta = csv.appendingPathExtension("jevmeta")
    let before = Data("original".utf8)
    try before.write(to: csv)
    let undo = UndoManager()
    undo.groupsByEvent = false
    #expect(throws: Failed.save) {
      try FileUndo.record([csv, meta], action: "Edit Table", in: undo) {
        try Data("changed".utf8).write(to: csv)
        try Data("new metadata".utf8).write(to: meta)
        throw Failed.save
      }
    }
    #expect(try Data(contentsOf: csv) == before)
    #expect(!files.fileExists(atPath: meta.path))
    #expect(!undo.canUndo)
  }

  @Test func undoAndRedoKeepASymbolicLinkAndThePermissions() throws {
    let files = FileManager.default
    let folder = files.temporaryDirectory.appending(path: "jevision-file-undo-\(UUID().uuidString)")
    defer { try? files.removeItem(at: folder) }
    let real = folder.appending(path: "shared/people.csv")
    let link = folder.appending(path: "people.csv")
    try files.createDirectory(
      at: real.deletingLastPathComponent(), withIntermediateDirectories: true)
    try writeDataRows(
      path: real.path(percentEncoded: false), table: DataRows(columns: ["name"], rows: [["Ann"]]))
    try files.setAttributes(
      [.posixPermissions: 0o640], ofItemAtPath: real.path(percentEncoded: false))
    try files.createSymbolicLink(
      atPath: link.path(percentEncoded: false), withDestinationPath: "shared/people.csv")
    let before = try Data(contentsOf: real)
    let undo = UndoManager()
    undo.groupsByEvent = false

    undo.beginUndoGrouping()
    try FileUndo.record(link, action: "Edit Table", in: undo, ask: { _, _ in false }) {
      try writeDataRows(
        path: link.path(percentEncoded: false), table: DataRows(columns: ["name"], rows: [["Bob"]]))
    }
    undo.endUndoGrouping()
    let after = try Data(contentsOf: real)
    #expect(after != before)

    func kept() throws {
      #expect(
        try files.destinationOfSymbolicLink(atPath: link.path(percentEncoded: false))
          == "shared/people.csv")
      #expect(
        try files.attributesOfItem(atPath: real.path(percentEncoded: false))[.posixPermissions]
          as? Int == 0o640)
    }
    undo.undo()
    #expect(try Data(contentsOf: real) == before)
    try kept()
    undo.redo()
    #expect(try Data(contentsOf: real) == after)
    try kept()
  }
}
