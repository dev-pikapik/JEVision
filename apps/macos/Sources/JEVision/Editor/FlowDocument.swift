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
  private(set) var layout = FlowLayout(width: 0, height: 0, blocks: [], frames: [], edges: [], slots: [])
  /// Why the last change couldn't be written to the file.
  private(set) var saveError: String?
  var selection: String? {
    didSet { if selection != oldValue { coalescing = nil } }
  }

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

  func insert(_ type: String, at slot: Slot?) {
    guard let flow, let edited = insertBlock(flow: flow, typeName: type, slot: slot) else { return }
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

  private func trashOnUndo(_ files: [URL], trash: @escaping Baseline.Trash) {
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

  func updateFlow(field: String, _ change: (inout Flow) -> Void) {
    guard var flow else { return }
    change(&flow)
    apply(flow, select: selection, action: String(localized: "Edit Test"), coalesce: "test." + field)
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
    issues = validateFlow(flow: new)
    layout = layoutFlow(flow: new)
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
