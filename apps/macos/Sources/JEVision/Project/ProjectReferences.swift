import Foundation
import JEVisionCore

/// A reference picture replaced together with the settings of the blocks that use it.
struct AcceptedReference {
  let screenshot: URL
  /// The reference picture file.
  let file: URL
  /// Where the old picture went in the Trash, `nil` when there was none.
  let old: URL?
  /// The settings written to tests; `nil` when no block's settings changed.
  let change: ReferenceChange?
}

/// Replacing a reference picture and changing the settings of the blocks that use it, in
/// every test, as one: when a part fails, nothing stays changed.
enum ReferenceAcceptance {
  /// Writes the `edits`, then puts `screenshot` in place of `file`; the old picture goes to the Trash.
  static func accept(
    _ screenshot: URL, as file: URL, in project: URL, edits: [ReferenceEdit], trash: Baseline.Trash = Baseline.moveToTrash
  ) throws -> AcceptedReference {
    let change = edits.isEmpty ? nil : try applyReferenceEdits(project: project.path(percentEncoded: false), edits: edits)
    do {
      let old = try Baseline.accept(screenshot, as: file, trash: trash)
      return AcceptedReference(screenshot: screenshot, file: file, old: old, change: change)
    } catch {
      if let change { try? undoReferenceEdits(project: project.path(percentEncoded: false), change: change) }
      throw error
    }
  }

  /// Takes `accepted` back: the tests return to their settings and the old picture to its place.
  /// A test changed since stays as it is, and then nothing is taken back.
  static func undo(_ accepted: AcceptedReference, in project: URL) throws {
    let path = project.path(percentEncoded: false)
    if let change = accepted.change { try undoReferenceEdits(project: path, change: change) }
    do {
      try Baseline.restore(accepted.old, to: accepted.file)
    } catch {
      if let change = accepted.change { try? redoReferenceEdits(project: path, change: change) }
      throw error
    }
  }

  /// Makes `accepted` again after `undo`.
  static func redo(
    _ accepted: AcceptedReference, in project: URL, trash: Baseline.Trash = Baseline.moveToTrash
  ) throws -> AcceptedReference {
    let path = project.path(percentEncoded: false)
    if let change = accepted.change { try redoReferenceEdits(project: path, change: change) }
    do {
      let old = try Baseline.accept(accepted.screenshot, as: accepted.file, trash: trash)
      return AcceptedReference(screenshot: accepted.screenshot, file: accepted.file, old: old, change: accepted.change)
    } catch {
      if let change = accepted.change { try? undoReferenceEdits(project: path, change: change) }
      throw error
    }
  }
}

extension ProjectStore {
  /// Opens the Accept New Reference wizard for the failed check `step` of the run in `run`:
  /// the blocks of every test that use its picture are looked up first.
  func startReferenceWizard(step: Step, run: URL) {
    guard let rootURL, let reference = step.reference else { return }
    let uses = referenceUses(project: rootURL.path(percentEncoded: false), reference: reference)
    referenceWizard = ReferenceWizard(step: step, run: run, uses: uses)
  }

  /// Makes `screenshot` the reference image `reference` (relative to the project). The old image
  /// goes to the Trash; Undo brings it back.
  func acceptReference(_ screenshot: URL, as reference: String, undoManager: UndoManager?) throws {
    try accept(screenshot, as: reference, edits: [], undoManager: undoManager)
  }

  /// Makes the wizard's screenshot the reference picture and gives the blocks the settings the
  /// user chose, in every test or in none. The old picture goes to the Trash; Undo brings back
  /// the picture and the settings.
  func acceptReference(_ wizard: ReferenceWizard, undoManager: UndoManager?) throws {
    try accept(wizard.screenshot, as: wizard.reference, edits: wizard.edits, undoManager: undoManager)
    wizard.finished()
  }

  private func accept(_ screenshot: URL, as reference: String, edits: [ReferenceEdit], undoManager: UndoManager?) throws {
    guard let rootURL, let file = Baseline.file(reference, in: rootURL) else {
      throw CocoaError(.fileWriteNoPermission, userInfo: [NSFilePathErrorKey: reference])
    }
    let accepted = try ReferenceAcceptance.accept(screenshot, as: file, in: rootURL, edits: edits)
    referencesChanged()
    registerUndo(of: accepted, in: rootURL, undoManager: undoManager)
  }

  private func registerUndo(of accepted: AcceptedReference, in root: URL, undoManager: UndoManager?) {
    let action = String(localized: "Accept as New Reference")
    undoManager?.registerUndo(withTarget: self) { store in
      MainActor.assumeIsolated {
        guard (try? ReferenceAcceptance.undo(accepted, in: root)) != nil else { return }
        store.referencesChanged()
        undoManager?.registerUndo(withTarget: store) { store in
          MainActor.assumeIsolated {
            guard let again = try? ReferenceAcceptance.redo(accepted, in: root) else { return }
            store.referencesChanged()
            store.registerUndo(of: again, in: root, undoManager: undoManager)
          }
        }
        undoManager?.setActionName(action)
      }
    }
    undoManager?.setActionName(action)
  }
}
