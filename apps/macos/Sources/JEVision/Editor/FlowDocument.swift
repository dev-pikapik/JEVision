import Foundation
import JEVisionCore
import Observation

/// One open test: the flow, its problems and layout, the selected block. Every change is saved
/// to the file right away and can be undone.
@MainActor
@Observable
final class FlowDocument {
  let url: URL
  private(set) var flow: Flow?
  /// Why the file can't be opened, when it can't.
  private(set) var failure: String?
  private(set) var issues: [Issue] = []
  private(set) var layout = FlowLayout(width: 0, height: 0, blocks: [], frames: [], edges: [], slots: [], ports: [])
  /// Why the last change couldn't be written to the file.
  private(set) var saveError: String?
  /// The block the inspector shows.
  var selection: String? {
    didSet {
      if selection != oldValue {
        coalescing = nil
        picked = selection.map { [.block(id: $0)] } ?? []
      }
    }
  }
  /// Everything selected on the canvas: the selected block and any other blocks, frames and notes.
  private(set) var picked: Set<CanvasItem> = []
  /// How the next runs go, picked in the Run menu.
  var runChoices = RunChoices()
  /// The run a report opened the test from: the blocks show how they did in it, not in the newest run.
  var reportRun: SavedRun.ID?

  @ObservationIgnored weak var undoManager: UndoManager?
  /// The text last read from or written to the file, to tell our own writes from changes made elsewhere.
  @ObservationIgnored private var savedText: String?
  /// Changes with the same key in a row, like typing into one field, undo as one step.
  @ObservationIgnored private var coalescing: String?

  init(url: URL) {
    self.url = url
    reload()
  }

  /// Reads the file again, keeping the selection if that block is still there.
  func reload() {
    let text: String
    do {
      text = try String(contentsOf: url, encoding: .utf8)
    } catch {
      flow = nil
      failure = error.localizedDescription
      return
    }
    guard text != savedText || flow == nil else { return }
    do {
      let loaded = try loadFlow(json: text)
      if flow != nil { undoManager?.removeAllActions(withTarget: self) }
      savedText = text
      failure = nil
      set(loaded)
      if let selection, block(selection) == nil { self.selection = nil }
    } catch FlowError.Invalid(let found) {
      flow = nil
      failure = found.map(IssueText.line).joined(separator: "\n")
    } catch {
      flow = nil
      failure = error.localizedDescription
    }
  }

  func block(_ id: String) -> Block? {
    flow?.blocks.first { $0.id == id }
  }

  func issues(of id: String) -> [Issue] {
    issues.filter { $0.param("block") == id }
  }

  /// Blocks in reading order: top to bottom, then left to right.
  var order: [String] {
    layout.blocks.sorted { ($0.rect.y, $0.rect.x) < ($1.rect.y, $1.rect.x) }.map(\.id)
  }

  // MARK: Changes

  /// Adds a block of `type` on `slot`; with `point`, puts it there on the canvas.
  func insert(_ type: String, at slot: Slot?, placedAt point: Point? = nil) {
    guard let flow, var edited = insertBlock(flow: flow, typeName: type, slot: slot) else { return }
    if let point, let id = edited.selection, let placed = placeBlock(flow: edited.flow, id: id, at: point) {
      edited.flow = placed
    }
    apply(edited.flow, select: edited.selection, action: String(localized: "Add Block"))
  }

  /// Adds what was recorded at `slot`, and the app it was recorded in when the test has none,
  /// as one change. Undo also moves `files`, the pictures the recording saved, to the Trash;
  /// Redo brings them back.
  func insertRecorded(
    _ blocks: [BlockKind], files: [URL], at slot: Slot?, app: String, trash: @escaping Baseline.Trash = Baseline.moveToTrash
  ) {
    guard var flow else { return }
    if flow.app.isEmpty { flow.app = app }
    guard let edited = insertBlocks(flow: flow, blocks: blocks, slot: slot) else { return }
    undoManager?.beginUndoGrouping()
    apply(edited.flow, select: edited.selection, action: String(localized: "Recording"))
    trashOnUndo(files, trash: trash)
    undoManager?.endUndoGrouping()
  }

  /// Makes Undo of the change just registered also move `files`, which it brought, to the Trash.
  func trashOnUndo(_ files: [URL], trash: @escaping Baseline.Trash = Baseline.moveToTrash) {
    undoManager?.registerUndo(withTarget: self) { document in
      MainActor.assumeIsolated {
        let trashed = files.map { try? trash($0) }
        document.undoManager?.registerUndo(withTarget: document) { document in
          MainActor.assumeIsolated {
            for case let (file, trashed?) in zip(files, trashed) { try? FileManager.default.moveItem(at: trashed, to: file) }
            document.trashOnUndo(files, trash: trash)
          }
        }
      }
    }
  }

  func delete(_ id: String) {
    guard let flow else { return }
    let edited = removeBlock(flow: flow, id: id)
    apply(edited.flow, select: edited.selection, action: String(localized: "Delete Block"))
  }

  /// Changes one block. `field` names what changed, so that typing into it undoes as one step.
  func update(_ id: String, field: String, _ change: (inout BlockKind) -> Void) {
    guard var flow, let index = flow.blocks.firstIndex(where: { $0.id == id }) else { return }
    change(&flow.blocks[index].kind)
    apply(flow, select: selection, action: String(localized: "Edit Block"), coalesce: id + "." + field)
  }

  /// Makes the next change a step of its own for Undo, even when it edits the same field.
  func endUndoStep() {
    coalescing = nil
  }

  func updateFlow(field: String, _ change: (inout Flow) -> Void) {
    guard var flow else { return }
    change(&flow)
    apply(flow, select: selection, action: String(localized: "Edit Test"), coalesce: "test." + field)
  }

  // MARK: Canvas

  /// Selects `items`. The inspector shows block `prefer` when it is one of them, otherwise the
  /// block already shown or the first one in reading order.
  func pick(_ items: Set<CanvasItem>, prefer: String? = nil) {
    let blocks = order.filter { items.contains(.block(id: $0)) }
    selection = [prefer, selection].compactMap { $0 }.first(where: blocks.contains) ?? blocks.first
    picked = items
  }

  /// The selected items: blocks in reading order, then frames and notes.
  var pickedInOrder: [CanvasItem] {
    order.map { CanvasItem.block(id: $0) }.filter(picked.contains)
      + picked.filter { $0.blockID == nil }.sorted { "\($0)" < "\($1)" }
  }

  /// Moves `items` by `dx`, `dy`, snapping the first of them to the grid.
  func move(_ items: [CanvasItem], dx: Double, dy: Double) {
    guard let flow, let moved = moveOnCanvas(flow: flow, items: items, dx: dx, dy: dy) else { return }
    apply(moved, select: selection, action: String(localized: "Move"))
  }

  func resize(_ item: CanvasItem, to rect: Rect) {
    guard let flow, let resized = resizeOnCanvas(flow: flow, item: item, rect: rect) else { return }
    apply(resized, select: selection, action: String(localized: "Resize"))
  }

  /// Lays the blocks out automatically again, forgetting where they were moved by hand.
  func arrange() {
    guard let flow else { return }
    apply(arrangeAutomatically(flow: flow), select: selection, action: String(localized: "Arrange Automatically"))
  }

  /// Points edge `edge` of block `from` at block `to`, or removes the link when `to` is `nil`.
  func connect(_ from: String, edge: String, to: String?) {
    guard let flow, let linked = connectBlocks(flow: flow, from: from, edge: edge, to: to) else { return }
    apply(linked, select: selection, action: to == nil ? String(localized: "Delete Link") : String(localized: "Link Blocks"))
  }

  /// Whether edge `edge` of block `from` can lead to block `to`: it doesn't already, and the
  /// link makes no loop outside a Repeat block.
  func canConnect(_ from: String, edge: String, to: String) -> Bool {
    flow.map { connectBlocks(flow: $0, from: from, edge: edge, to: to) != nil } ?? false
  }

  /// Adds an empty note with its top-left corner at `point`, signed by `author`, and selects it.
  func addNote(at point: Point, author: String) {
    guard let flow else { return }
    let edit = JEVisionCore.addNote(flow: flow, text: "", author: author, time: Date.now.ISO8601Format(), at: point)
    apply(edit.flow, select: nil, action: String(localized: "Add Note"))
    pick(Set(edit.items))
  }

  /// Adds a frame with the bounds `rect` and selects it.
  func addFrame(_ rect: Rect) {
    guard let flow else { return }
    let edit = addSection(flow: flow, title: String(localized: "New Frame"), rect: rect)
    apply(edit.flow, select: nil, action: String(localized: "Add Frame"))
    pick(Set(edit.items))
  }

  func editNote(_ id: String, text: String) {
    guard var flow, let index = flow.canvas.notes.firstIndex(where: { $0.id == id }) else { return }
    flow.canvas.notes[index].text = text
    apply(flow, select: selection, action: String(localized: "Edit Note"), coalesce: "note." + id)
  }

  func editFrame(_ id: String, title: String) {
    guard var flow, let index = flow.canvas.sections.firstIndex(where: { $0.id == id }) else { return }
    flow.canvas.sections[index].title = title
    apply(flow, select: selection, action: String(localized: "Rename Frame"), coalesce: "frame." + id)
  }

  /// Deletes `items`; deleted blocks are bridged, so the blocks around them stay linked.
  func remove(_ items: [CanvasItem]) {
    guard let flow, !items.isEmpty else { return }
    let edited = removeFromCanvas(flow: flow, items: items)
    apply(edited.flow, select: edited.selection, action: String(localized: "Delete"))
  }

  /// The selection as the text of a test file, for the clipboard; empty when nothing is selected.
  func copyPicked() -> String {
    guard let flow, !picked.isEmpty else { return "" }
    return copyFromCanvas(flow: flow, items: pickedInOrder)
  }

  /// Adds the copied `text` after the selected block, or beside the originals when no block is
  /// selected, and selects what was added. Returns `false` when `text` isn't a copied test.
  @discardableResult
  func paste(_ text: String, action: String = String(localized: "Paste")) -> Bool {
    guard let flow, let edit = pasteOnCanvas(flow: flow, text: text, after: selection) else { return false }
    apply(edit.flow, select: selection, action: action)
    pick(Set(edit.items))
    return true
  }

  func duplicate() {
    let text = copyPicked()
    if !text.isEmpty { paste(text, action: String(localized: "Duplicate")) }
  }

  private func apply(_ new: Flow, select: String?, action: String, coalesce: String? = nil) {
    guard let old = flow, new != old else { return }
    if coalesce == nil || coalesce != coalescing, let undoManager {
      let oldSelection = selection
      undoManager.registerUndo(withTarget: self) { document in
        MainActor.assumeIsolated { document.apply(old, select: oldSelection, action: action) }
      }
      undoManager.setActionName(action)
    }
    set(new)
    selection = select
    coalescing = coalesce
    save()
  }

  private func set(_ new: Flow) {
    flow = new
    issues = check(new)
    layout = layoutFlow(flow: new)
    picked = picked.filter { item in
      switch item {
      case .block(let id): new.blocks.contains { $0.id == id }
      case .section(let id): new.canvas.sections.contains { $0.id == id }
      case .note(let id): new.canvas.notes.contains { $0.id == id }
      }
    }
  }

  /// Checks the test again after something it depends on changed, like the project's environments.
  func revalidate() {
    if let flow { issues = check(flow) }
  }

  private func check(_ flow: Flow) -> [Issue] {
    validateFlow(flow: flow) + environmentIssues(flow: flow, folder: url.deletingLastPathComponent().path(percentEncoded: false))
  }

  private func save() {
    guard let flow else { return }
    let text = saveFlow(flow: flow)
    do {
      try text.write(to: url, atomically: true, encoding: .utf8)
      savedText = text
      saveError = nil
    } catch {
      saveError = error.localizedDescription
    }
  }
}

extension CanvasItem {
  var blockID: String? {
    if case .block(let id) = self { id } else { nil }
  }
}
