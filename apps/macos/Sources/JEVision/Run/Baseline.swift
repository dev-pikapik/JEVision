import Foundation

/// Making a run's screenshot the reference image a Check compares with.
enum Baseline {
  /// Moves a file to the Trash and returns where it went there.
  typealias Trash = (URL) throws -> URL

  static func moveToTrash(_ url: URL) throws -> URL {
    var trashed: NSURL?
    try FileManager.default.trashItem(at: url, resultingItemURL: &trashed)
    return (trashed as URL?) ?? url
  }

  /// The reference file a report names, relative to the project; `nil` if it points outside it.
  static func file(_ reference: String, in project: URL) -> URL? {
    let root = project.standardizedFileURL.path(percentEncoded: false)
    let url = project.appending(path: reference).standardizedFileURL
    let prefix = root.hasSuffix("/") ? root : root + "/"
    return url.path(percentEncoded: false).hasPrefix(prefix) ? url : nil
  }

  /// Puts `screenshot` in place of `reference`. The old reference goes to the Trash, so it can
  /// come back; returns where it went, or `nil` when there was none.
  static func accept(_ screenshot: URL, as reference: URL, trash: Trash = moveToTrash) throws -> URL? {
    let files = FileManager.default
    let old = files.fileExists(atPath: reference.path) ? try trash(reference) : nil
    do {
      try files.createDirectory(at: reference.deletingLastPathComponent(), withIntermediateDirectories: true)
      try files.copyItem(at: screenshot, to: reference)
    } catch {
      if let old { try? files.moveItem(at: old, to: reference) }
      throw error
    }
    return old
  }

  /// Takes back `accept`: the accepted screenshot goes away and the old reference returns from the Trash.
  static func restore(_ old: URL?, to reference: URL) throws {
    let files = FileManager.default
    if files.fileExists(atPath: reference.path) { try files.removeItem(at: reference) }
    if let old { try files.moveItem(at: old, to: reference) }
  }

  /// The screenshot already is the reference: the same bytes.
  static func isReference(_ screenshot: URL, _ reference: URL) -> Bool {
    FileManager.default.contentsEqual(atPath: screenshot.path, andPath: reference.path)
  }
}
