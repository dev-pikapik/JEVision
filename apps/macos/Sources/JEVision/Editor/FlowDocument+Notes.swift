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

  private var now: String {
    Date.now.ISO8601Format()
  }
}
