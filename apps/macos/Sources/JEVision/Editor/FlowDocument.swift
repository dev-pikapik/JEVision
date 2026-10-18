import Foundation
import JEVisionCore
import Observation

/// One open test: the flow, its problems and layout, the selected block. Every change is saved
/// to the file right away and can be undone.
@MainActor
@Observable
final class FlowDocument {
  let url: URL
  private(set) var flow: Flow? {
    didSet {
      blockIndex = Dictionary(
        (flow?.blocks ?? []).enumerated().map { ($1.id, $0) }, uniquingKeysWith: { first, _ in first })
    }
  }
  /// Why the file can't be opened, when it can't.
  private(set) var failure: String?
  private(set) var issues: [Issue] = [] {
    didSet { blockIssues = Dictionary(grouping: issues) { $0.param("block") } }
  }
  private(set) var layout = FlowLayout(width: 0, height: 0, blocks: [], frames: [], edges: [], slots: [], ports: []) {
    didSet {
      order = layout.blocks.sorted { ($0.rect.y, $0.rect.x) < ($1.rect.y, $1.rect.x) }.map(\.id)
      steps = Dictionary(order.enumerated().map { ($1, $0 + 1) }, uniquingKeysWith: { first, _ in first })
      linked = Set(
        layout.edges.filter { $0.from != $0.to }.flatMap { $0.edge == "body" ? [$0.to] : [$0.from, $0.to] })
    }
  }
  /// Blocks in reading order: top to bottom, then left to right.
  private(set) var order: [String] = []
  // Worked out once per change, so the canvas doesn't search the whole test for every block it
  // draws: in a test of hundreds of blocks that made every frame of a drag slow.
  @ObservationIgnored private var blockIndex: [String: Int] = [:]
  private var blockIssues: [String?: [Issue]] = [:]
  private var steps: [String: Int] = [:]
  private var linked: Set<String> = []
  /// Why the last change couldn't be written to the file.
  private(set) var saveError: String?
  /// The block the inspector shows.
  var selection: String? {
    didSet {
      if selection != oldValue {
        coalescing = nil
        picked = selection.map { [.block(id: $0)] } ?? []
        pickedLinks = []
        linking = nil
      }
    }
  }
  /// Everything selected on the canvas: the selected block and any other blocks, frames and notes.
  private(set) var picked: Set<CanvasItem> = []
  /// Links selected on the canvas, each named by the edge it leaves its block from.
  private(set) var pickedLinks: Set<Slot> = []
  /// A link being made from the keyboard.
  private(set) var linking: PendingLink?
  /// How the next runs go, picked in the Run menu.
  var runChoices = RunChoices()
  /// The run a report opened the test from: the blocks show how they did in it, not in the newest run.
  var reportRun: SavedRun.ID?

  @ObservationIgnored weak var undoManager: UndoManager?
  @ObservationIgnored private let project: URL?
  /// The text last read from or written to the file, to tell our own writes from changes made elsewhere.
  @ObservationIgnored private var savedText: String?
  /// Changes with the same key in a row, like typing into one field, undo as one step.
  @ObservationIgnored private var coalescing: String?

  /// `project` is the folder the tests it depends on are looked up in; without one they aren't checked.
  init(url: URL, project: URL? = nil) {
    self.url = url
    self.project = project
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
    guard let flow, let index = blockIndex[id] else { return nil }
    return flow.blocks[index]
  }

  func issues(of id: String) -> [Issue] {
    blockIssues[id] ?? []
  }

  /// Where block `id` comes in reading order, counted from 1; 1 for a block not laid out.
  func step(of id: String) -> Int {
    steps[id] ?? 1
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

  /// Where a block added now goes: after the selected block, or at the end without one.
  var slotAfterSelection: Slot? {
    guard let flow, let selection else { return nil }
    return slotAfterBlock(flow: flow, id: selection)
  }

  /// One more condition on the selected If, otherwise a new If after the selected block.
  func addCondition() {
    guard let flow, let edited = addConditionAt(flow: flow, selection: selection) else { return }
    let grows = selection != nil && selection == edited.selection
    apply(
      edited.flow, select: edited.selection,
      action: grows ? String(localized: "Add Condition") : String(localized: "Add Block"))
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
    pickedLinks = []
    linking = nil
  }

  /// Selects links `links` and nothing else.
  func pickLinks(_ links: Set<Slot>) {
    selection = nil
    picked = []
    pickedLinks = links
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

  /// Whether a link leads into or out of block `id`, other than a loop's own body.
  func hasLinks(_ id: String) -> Bool {
    linked.contains(id)
  }

  /// Removes every link into and out of block `id`, so it stands alone.
  func disconnect(_ id: String) {
    guard let flow, let edited = disconnectBlock(flow: flow, id: id) else { return }
    apply(edited, select: id, action: String(localized: "Disconnect"))
  }

  /// Deletes links `links` as one step.
  func unlink(_ links: [Slot]) {
    guard let flow, !links.isEmpty else { return }
    let edited = links.reduce(flow) { flow, link in
      connectBlocks(flow: flow, from: link.from, edge: link.edge, to: nil) ?? flow
    }
    apply(
      edited, select: selection,
      action: links.count == 1 ? String(localized: "Delete Link") : String(localized: "Delete Links"))
    pickedLinks = []
  }

  /// Starts making a link from the keyboard, from edge `slot`; without one, from an edge of the
  /// selected block, the next one each time while a link from it is being made. The block it
  /// would lead to starts as the nearest block it may lead to. Returns `false` when there is no
  /// such block.
  @discardableResult
  func startLink(from slot: Slot? = nil) -> Bool {
    let ports = layout.ports.map(\.slot).filter { $0.from == slot?.from ?? selection }
    let current = linking.flatMap { ports.firstIndex(of: $0.slot) }
    guard let slot = slot ?? (current.map { ports[($0 + 1) % ports.count] } ?? ports.first),
      let from = layout.blocks.first(where: { $0.id == slot.from })
    else { return false }
    let targets = layout.blocks.filter { canConnect(slot.from, edge: slot.edge, to: $0.id) }
    guard
      let nearest = targets.min(by: {
        CanvasNavigation.distance($0.rect, from.rect) < CanvasNavigation.distance($1.rect, from.rect)
      })
    else { return false }
    if selection != slot.from { pick([.block(id: slot.from)], prefer: slot.from) }
    linking = PendingLink(slot: slot, targets: targets.map(\.id), target: nearest.id)
    return true
  }

  /// Moves the block the link being made would lead to, to the nearest one it may lead to that way.
  func moveLinkTarget(_ direction: CanvasDirection) {
    guard var link = linking else { return }
    let candidates = layout.blocks.filter { link.targets.contains($0.id) }
    guard let from = layout.blocks.first(where: { $0.id == link.target }),
      let next = CanvasNavigation.nearest(from: from.rect, among: candidates, toward: direction)
    else { return }
    link.target = next
    linking = link
  }

  /// Ends the link being made at block `target`, or at the block it leads to now. Returns `false`,
  /// keeping the link going, when it may not lead there.
  @discardableResult
  func finishLink(at target: String? = nil) -> Bool {
    guard let link = linking, let target = target ?? link.target, link.targets.contains(target) else { return false }
    linking = nil
    connect(link.slot.from, edge: link.slot.edge, to: target)
    return true
  }

  func cancelLink() {
    linking = nil
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
    pickedLinks = pickedLinks.filter { link in layout.edges.contains { $0.from == link.from && $0.edge == link.edge } }
    if let linking, !layout.ports.contains(where: { $0.slot == linking.slot }) { self.linking = nil }
  }

  /// Checks the test again after something it depends on changed, like the project's environments.
  func revalidate() {
    if let flow { issues = check(flow) }
  }

  private func check(_ flow: Flow) -> [Issue] {
    let folder = url.deletingLastPathComponent().path(percentEncoded: false)
    var issues = validateFlow(flow: flow) + environmentIssues(flow: flow, folder: folder) + dataIssues(flow: flow, folder: folder)
    if let project, RelativePath.isInside(url, folder: project) {
      issues += dependencyIssues(
        flow: flow, project: project.path(percentEncoded: false), file: RelativePath.from(project, to: url))
    }
    return issues
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

/// A link being made from the keyboard.
struct PendingLink: Equatable {
  /// The edge the link leaves its block from.
  let slot: Slot
  /// Blocks the link may lead to.
  let targets: [String]
  /// The block it leads to when finished now.
  var target: String?
}
