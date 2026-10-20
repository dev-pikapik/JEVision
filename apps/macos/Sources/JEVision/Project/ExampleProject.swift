import Foundation

/// The example project that ships inside the app: a test of TextEdit, which every Mac has. The
/// bundle is never changed: a run saves reports and pictures next to the tests, so the person
/// opens a copy.
enum ExampleProject {
  static let name = "JEVision Example"
  /// The test to show first, relative to the example's folder.
  static let firstTest = "write-a-note.flow.json"

  enum Failure: LocalizedError {
    case missing

    var errorDescription: String? {
      String(localized: "This copy of JEVision doesn’t include the example project.")
    }
  }

  /// The example inside the app; `nil` when the app runs without a bundle, as from `swift run`.
  static func bundled(in bundle: Bundle = .main) -> URL? {
    guard let url = bundle.resourceURL?.appending(path: "examples/textedit", directoryHint: .isDirectory),
      FileManager.default.fileExists(atPath: url.path)
    else { return nil }
    return url
  }

  /// Copies `source` into a new folder of `parent`, named after the example and numbered if that
  /// name is taken, and returns the copy.
  static func copy(_ source: URL, into parent: URL) throws -> URL {
    let folder = RelativePath.unique(name) {
      FileManager.default.fileExists(atPath: parent.appending(path: $0).path)
    }
    let copy = parent.appending(path: folder, directoryHint: .isDirectory)
    try FileManager.default.copyItem(at: source, to: copy)
    return copy
  }
}
