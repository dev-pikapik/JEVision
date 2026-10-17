import Foundation

/// Paths in a test are written relative to the test file's folder, with `/`.
enum RelativePath {
  /// `/p/tests/refs/a.png` from `/p/tests` → `refs/a.png`; from `/p/tests/x` → `../refs/a.png`.
  static func from(_ folder: URL, to file: URL) -> String {
    let base = folder.standardizedFileURL.pathComponents
    let target = file.standardizedFileURL.pathComponents
    let shared = zip(base, target).prefix { $0 == $1 }.count
    let up = Array(repeating: "..", count: base.count - shared)
    return (up + target[shared...]).joined(separator: "/")
  }

  static func resolve(_ path: String, in folder: URL) -> URL {
    folder.appending(path: path).standardizedFileURL
  }

  static func isInside(_ file: URL, folder: URL) -> Bool {
    let root = folder.standardizedFileURL.pathComponents
    let parts = file.standardizedFileURL.pathComponents
    return parts.count > root.count && Array(parts.prefix(root.count)) == root
  }

  /// A file name for a test called `name`: characters a file name can't hold become dashes.
  static func testFileName(_ name: String) -> String {
    let clean = name.trimmingCharacters(in: .whitespaces)
      .replacing(/[\/:\\]/, with: "-")
      .trimmingCharacters(in: CharacterSet(charactersIn: "."))
    return (clean.isEmpty ? String(localized: "Untitled") : clean) + ProjectScanner.testSuffix
  }

  /// `name`, or `name 2`, `name 3`… — the first that `exists` says is free.
  static func unique(_ name: String, exists: (String) -> Bool) -> String {
    guard exists(name) else { return name }
    return (2...).lazy.map { "\(name) \($0)" }.first { !exists($0) }!
  }
}
