import AppKit

/// Undo and Redo of a change written to a file of the project, like the environments or a data
/// table: they put back what the file held before the change, or after it. Something else may have
/// changed the file since — another window, an editor, a pull — and then nothing is replaced unless
/// the person says so.
@MainActor
enum FileUndo {
  /// Asks whether to replace a file changed since `action`; true replaces it.
  typealias Ask = @MainActor (_ file: URL, _ action: String) -> Bool

  /// Runs `change`, which writes `file`, and makes it one step for Undo named `action`. After Undo
  /// or Redo writes the file, `then` runs, to show what it holds now.
  static func record(
    _ file: URL, action: String, in undoManager: UndoManager?, ask: @escaping Ask = askToReplace,
    then: @escaping () -> Void = {}, _ change: () throws -> Void
  ) rethrows {
    let before = contents(of: file)
    try change()
    let after = contents(of: file)
    guard let undoManager, before != after else { return }
    register(file, back: before, from: after, action: action, in: undoManager, ask: ask, then: then)
  }

  /// Related files change and undo together, such as a CSV table and the label that describes it.
  /// A failed change restores their original contents before the error reaches the caller.
  static func record(
    _ files: [URL], action: String, in undoManager: UndoManager?, ask: @escaping Ask = askToReplace,
    beforeRestore: @escaping () throws -> Void = {}, then: @escaping () -> Void = {},
    _ change: () throws -> Void
  ) throws {
    let before = try files.map { try snapshot(of: $0) }
    let after: [Data?]
    do {
      try change()
      after = try files.map { try snapshot(of: $0) }
    } catch {
      try restore(before, to: files)
      throw error
    }
    guard let undoManager, before != after else { return }
    register(
      files, back: before, from: after, action: action, in: undoManager, ask: ask,
      check: beforeRestore, then: then)
  }

  private static func snapshot(of file: URL) throws -> Data? {
    do { return try Data(contentsOf: file) } catch CocoaError.fileReadNoSuchFile { return nil }
  }

  private static func restore(_ contents: [Data?], to files: [URL]) throws {
    var first: (any Error)?
    for (file, data) in zip(files, contents) {
      do { try write(data, to: file) } catch { first = first ?? error }
    }
    if let first { throw first }
  }

  private static func register(
    _ files: [URL], back: [Data?], from: [Data?], action: String, in undoManager: UndoManager,
    ask: @escaping Ask, check: @escaping () throws -> Void, then: @escaping () -> Void
  ) {
    let target = Step()
    undoManager.registerUndo(withTarget: target) { [target, weak undoManager] _ in
      MainActor.assumeIsolated {
        _ = target
        guard let undoManager else { return }
        do {
          let now = try files.map { try snapshot(of: $0) }
          try check()
          for (index, file) in files.enumerated() where now[index] != from[index] {
            guard ask(file, action) else { return }
          }
          do {
            try restore(back, to: files)
          } catch {
            try restore(now, to: files)
            throw error
          }
          then()
          register(
            files, back: now, from: back, action: action, in: undoManager, ask: ask, check: check,
            then: then)
        } catch {
          if let file = files.first { report(error, file: file) }
        }
      }
    }
    undoManager.setActionName(action)
  }

  /// What the file holds; `nil` when there is no file.
  static func contents(of file: URL) -> Data? {
    try? Data(contentsOf: file)
  }

  /// Registers putting `back` into the file while it holds `from`; the step it registers in turn
  /// puts back whatever it replaced.
  private static func register(
    _ file: URL, back: Data?, from: Data?, action: String, in undoManager: UndoManager,
    ask: @escaping Ask,
    then: @escaping () -> Void
  ) {
    let target = Step()
    // The undo manager keeps the step, so the step mustn't keep the undo manager.
    undoManager.registerUndo(withTarget: target) { [target, weak undoManager] _ in
      MainActor.assumeIsolated {
        _ = target
        guard let undoManager else { return }
        let now = contents(of: file)
        guard now == from || ask(file, action) else { return }
        do {
          try write(back, to: file)
        } catch {
          report(error, file: file)
          return
        }
        then()
        register(file, back: now, from: back, action: action, in: undoManager, ask: ask, then: then)
      }
    }
    undoManager.setActionName(action)
  }

  /// Writes in one go, like the core does: a symbolic link keeps pointing where it did, and the file
  /// keeps its permissions.
  private static func write(_ data: Data?, to file: URL) throws {
    let files = FileManager.default
    let path = file.resolvingSymlinksInPath().path(percentEncoded: false)
    if let data {
      let permissions = try? files.attributesOfItem(atPath: path)[.posixPermissions]
      try data.write(to: URL(filePath: path), options: .atomic)
      if let permissions {
        try? files.setAttributes([.posixPermissions: permissions], ofItemAtPath: path)
      }
    } else if files.fileExists(atPath: path) {
      try files.removeItem(atPath: path)
    }
  }

  /// The question shown when the file changed after the step being undone or redone.
  static func askToReplace(_ file: URL, action: String) -> Bool {
    let alert = NSAlert()
    alert.alertStyle = .warning
    alert.messageText = String(
      localized: "“\(file.lastPathComponent)” has changed since “\(action)”")
    alert.informativeText = String(
      localized:
        "It was changed somewhere else afterwards. Going back now would replace those changes.")
    alert.addButton(withTitle: String(localized: "Keep the File as It Is"))
    alert.addButton(withTitle: String(localized: "Replace the Changes"))
    alert.buttons.last?.hasDestructiveAction = true
    return alert.runModal() == .alertSecondButtonReturn
  }

  private static func report(_ error: any Error, file: URL) {
    let alert = NSAlert(error: error)
    alert.messageText = String(localized: "“\(file.lastPathComponent)” couldn’t be changed back")
    alert.runModal()
  }

  /// What a step is registered on, so each step can be told apart from others on the same file,
  /// and removing the steps of a document or a store leaves it.
  final class Step {}
}
