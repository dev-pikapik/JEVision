import Foundation
import JEVisionCore

/// What the editor does with the notes of a test: who they are about, the talk under them, the
/// files and links they bring. Each change is one step for Undo and is saved at once.
extension FlowDocument {
  func note(_ id: String) -> Note? {
    flow?.canvas.notes.first { $0.id == id }
  }

  /// The notes about block `block`, in the order they are in the file.
  func notes(about block: String) -> [Note] {
    (flow?.canvas.notes ?? []).filter { $0.anchor == .block(id: block) }
  }

  /// The note the selection is, when it is nothing else.
  var pickedNote: Note? {
    guard picked.count == 1, case .note(let id)? = picked.first else { return nil }
    return note(id)
  }

  var hasResolvedNotes: Bool {
    flow?.canvas.notes.contains { $0.resolved != nil } ?? false
  }

  /// Whether the canvas leaves `note` out: it is settled, the filter is on and it isn't selected.
  func isHidden(_ note: Note) -> Bool {
    hidesResolvedNotes && note.resolved != nil && !picked.contains(.note(id: note.id))
  }

  /// Who notes and replies are signed by: the person git knows, else the macOS account.
  var signature: String {
    author.isEmpty ? NSFullUserName() : author
  }

  /// Adds an empty note about block `block` beside it.
  func addNote(about block: String, select: Bool = true) {
    guard let at = spot(besideBlock: block) else { return }
    addNote(at: at, author: signature, anchor: .block(id: block), select: select)
  }

  /// A place for a new note to the right of block `block`, moved down while something is there.
  private func spot(besideBlock block: String) -> Point? {
    guard let rect = layout.blocks.first(where: { $0.id == block })?.rect else { return nil }
    let grid = FlowCanvas.grid
    let size = CGSize(width: 14 * grid, height: 9 * grid)
    let taken = layout.blocks.map(\.rect.cgRect) + (flow?.canvas.notes ?? []).map(\.rect.cgRect)
    var origin = CGPoint(x: rect.x + rect.width + 2 * grid, y: rect.y)
    var moves = 0
    while moves < 60, taken.contains(where: { $0.intersects(CGRect(origin: origin, size: size)) }) {
      origin.y += 2 * grid
      moves += 1
    }
    return Point(x: origin.x, y: origin.y)
  }

  func setAnchor(of note: String, to anchor: NoteAnchor) {
    guard let flow else { return }
    change(
      noteSetAnchor(flow: flow, note: note, anchor: anchor), action: String(localized: "Change What the Note Is About"))
  }

  /// Adds a reply written by `author` under `note`; returns its id, or `nil` for an empty text.
  @discardableResult
  func reply(to note: String, author: String, text: String) -> String? {
    guard let flow, let added = noteAddReply(flow: flow, note: note, author: author, time: now, text: text) else {
      return nil
    }
    change(added.flow, action: String(localized: "Add Reply"))
    return added.reply
  }

  func editReply(_ reply: String, of note: String, text: String) {
    guard let flow else { return }
    change(
      noteEditReply(flow: flow, note: note, reply: reply, text: text), action: String(localized: "Edit Reply"),
      coalesce: "reply." + reply)
  }

  func deleteReply(_ reply: String, of note: String) {
    guard let flow else { return }
    change(noteDeleteReply(flow: flow, note: note, reply: reply), action: String(localized: "Delete Reply"))
  }

  func resolve(_ note: String, by author: String) {
    guard let flow else { return }
    change(noteResolve(flow: flow, note: note, by: author, time: now), action: String(localized: "Resolve Note"))
  }

  func reopen(_ note: String) {
    guard let flow else { return }
    change(noteReopen(flow: flow, note: note), action: String(localized: "Reopen Note"))
  }

  func attach(_ attachment: Attachment, to note: String) {
    guard let flow else { return }
    change(noteAttach(flow: flow, note: note, attachment: attachment), action: String(localized: "Attach to Note"))
  }

  func detach(_ attachment: Attachment, from note: String) {
    guard let flow else { return }
    change(noteDetach(flow: flow, note: note, attachment: attachment), action: String(localized: "Remove from Note"))
  }

  /// The files in the test's notes folder that no note holds any more; `nil` when there are none,
  /// and while the canvas shows something that isn't in the file, like a draft.
  var unusedNoteFiles: UnusedNoteFiles? {
    guard let flow, !isReadOnly, !hasUnsavedChanges else { return nil }
    let unused = noteUnusedFiles(test: url.path(percentEncoded: false), flow: flow)
    return unused.paths.isEmpty ? nil : unused
  }

  /// Moves `unused` to the Trash and drops the lines of `.gitattributes` in `project` that sent
  /// only them to Git LFS, as one step that Undo takes back. `then` runs after Undo or Redo moves
  /// the files. Throws the first file that couldn't be moved, the others are moved all the same;
  /// a ``GitAttributesFailure`` when the lines couldn't be dropped, which tells both.
  func removeUnusedNoteFiles(
    _ unused: UnusedNoteFiles, project: URL?, trash: @escaping Baseline.Trash = Baseline.moveToTrash,
    ask: @escaping FileUndo.Ask = FileUndo.askToReplace, then: @escaping () -> Void = {}
  ) throws {
    let files = unused.paths.map { URL(filePath: $0) }
    // Taken while the files are there: for a project opened as `/private/tmp/…`, the path of a file
    // that's gone keeps `/private` and the project's loses it, so it would lead out of the project.
    let paths = project.map { project in files.map { RelativePath.from(project, to: $0) } } ?? []
    let action = String(localized: "Remove Unused Note Files")
    undoManager?.beginUndoGrouping()
    defer {
      undoManager?.setActionName(action)
      undoManager?.endUndoGrouping()
    }
    let failure = trashNoteFiles(files, trash: trash, then: then)
    var untrack: (any Error)?
    if let project {
      let gone = zip(files, paths).filter { file, _ in !FileManager.default.fileExists(atPath: file.path(percentEncoded: false)) }
        .map(\.1)
      do {
        try FileUndo.record(project.appending(path: ".gitattributes"), action: action, in: undoManager, ask: ask) {
          do {
            try gitLfsUntrack(project: project.path(percentEncoded: false), paths: gone)
          } catch GitError.NotInstalled, GitError.NotRepository {
            // Without Git there are no lines to drop, and a line left behind names a file that isn't there.
          }
        }
      } catch {
        untrack = error
      }
    }
    if let untrack {
      throw GitAttributesFailure(reason: GitText.failure(untrack), moving: failure?.localizedDescription)
    }
    if let failure { throw failure }
  }

  /// The files went to the Trash, but their lines in `.gitattributes` stayed; `moving` says why
  /// a file didn't go, when one didn't, so the person learns both from one alert.
  struct GitAttributesFailure: LocalizedError {
    let reason: String
    var moving: String?

    var errorDescription: String? {
      moving ?? String(localized: "The files are in the Trash, but .gitattributes still lists them")
    }

    var recoverySuggestion: String? {
      let why =
        moving == nil
        ? reason : String(localized: "The files that went to the Trash are still listed in .gitattributes: \(reason)")
      return String(
        localized: "\(why)\n\nRemove their lines from .gitattributes yourself, or choose Edit › Undo to put the files back and try again.")
    }
  }

  /// Moves `files` to the Trash; Undo puts back those that went, and Redo moves them again.
  /// Returns why a file couldn't be moved, the first one.
  private func trashNoteFiles(_ files: [URL], trash: @escaping Baseline.Trash, then: @escaping () -> Void) -> (any Error)? {
    var moved: [(file: URL, trashed: URL)] = []
    var failure: (any Error)?
    for file in files {
      do {
        moved.append((file, try trash(file)))
      } catch {
        failure = failure ?? error
      }
    }
    guard !moved.isEmpty else { return failure }
    // Registered on a step of its own, not on the document: reading the test again drops the steps
    // that edited it and must leave this one, or Undo would bring back the lines of .gitattributes
    // without the files. The undo manager doesn't keep its targets, so the closures keep the step.
    let step = FileUndo.Step()
    undoManager?.registerUndo(withTarget: step) { [self, step] _ in
      MainActor.assumeIsolated {
        for (file, trashed) in moved {
          try? FileManager.default.createDirectory(
            at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
          try? FileManager.default.moveItem(at: trashed, to: file)
        }
        undoManager?.registerUndo(withTarget: step) { [self, step] _ in
          MainActor.assumeIsolated {
            _ = step
            _ = trashNoteFiles(moved.map(\.file), trash: trash, then: then)
            then()
          }
        }
        then()
      }
    }
    return failure
  }

  private var now: String {
    Date.now.ISO8601Format()
  }
}
