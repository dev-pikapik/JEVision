import Foundation
import JEVisionCore

/// A picture in a temporary file, deleted when nothing holds it any more. Redo needs the new
/// picture again, so whatever can redo holds it.
final class TemporaryPicture {
  let url: URL

  init(_ png: Data) throws {
    url = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString + ".png")
    try png.write(to: url)
  }

  deinit { try? FileManager.default.removeItem(at: url) }
}

/// A reference picture replaced together with the settings of the blocks that use it.
struct AcceptedReference {
  let screenshot: URL
  /// Keeps the file of `screenshot` when it is temporary.
  let keeping: TemporaryPicture?
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
  /// `keeping` holds the file of `screenshot` for as long as the result is kept.
  static func accept(
    _ screenshot: URL, keeping: TemporaryPicture? = nil, as file: URL, in project: URL, edits: [ReferenceEdit],
    trash: Baseline.Trash = Baseline.moveToTrash
  ) throws -> AcceptedReference {
    let change = edits.isEmpty ? nil : try applyReferenceEdits(project: project.path(percentEncoded: false), edits: edits)
    do {
      let old = try Baseline.accept(screenshot, as: file, trash: trash)
      return AcceptedReference(screenshot: screenshot, keeping: keeping, file: file, old: old, change: change)
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
      return AcceptedReference(
        screenshot: accepted.screenshot, keeping: accepted.keeping, file: accepted.file, old: old, change: accepted.change)
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

  /// Makes `picture` the reference image `reference` (relative to the project). The old image
  /// goes to the Trash; Undo brings it back.
  func acceptReference(_ picture: TemporaryPicture, as reference: String, undoManager: UndoManager?) throws {
    try accept(picture.url, keeping: picture, as: reference, edits: [], undoManager: undoManager)
  }

  /// The Accept New Reference guide for `picture`, chosen in the reference editor of `block` in
  /// the test `file`, to replace `reference`; `nil` when no other block uses `reference`, and
  /// then the picture can be replaced right away.
  func referenceWizard(
    replacing reference: String, with picture: TemporaryPicture, in file: URL, block: String
  ) -> ReferenceWizard? {
    guard let rootURL else { return nil }
    return ReferenceWizard(replacing: reference, with: picture, in: rootURL, file: file, block: block)
  }

  /// Makes the wizard's screenshot the reference picture and gives the blocks the settings the
  /// user chose, in every test or in none. The old picture goes to the Trash; Undo brings back
  /// the picture and the settings.
  func acceptReference(_ wizard: ReferenceWizard, undoManager: UndoManager?) throws {
    try accept(
      wizard.screenshot, keeping: wizard.keeping, as: wizard.reference, edits: wizard.edits, undoManager: undoManager)
    wizard.finished()
  }

  private func accept(
    _ screenshot: URL, keeping: TemporaryPicture? = nil, as reference: String, edits: [ReferenceEdit],
    undoManager: UndoManager?
  ) throws {
    guard let rootURL, let file = Baseline.file(reference, in: rootURL) else {
      throw CocoaError(.fileWriteNoPermission, userInfo: [NSFilePathErrorKey: reference])
    }
    let accepted = try ReferenceAcceptance.accept(screenshot, keeping: keeping, as: file, in: rootURL, edits: edits)
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
