import Foundation

/// The example project that ships inside the app: a test of TextEdit, which every Mac has. The
/// bundle is never changed: a run saves reports and pictures next to the tests, so the New
/// Project sheet makes a project that is a copy of it.
enum ExampleProject {
  static let name = "JEVision Example"
  /// The test to show first, relative to the example's folder.
  static let firstTest = "write-a-note.flow.json"

  /// The example inside the app; `nil` when the app runs without a bundle, as from `swift run`.
  static func bundled(in bundle: Bundle = .main) -> URL? {
    guard let url = bundle.resourceURL?.appending(path: "examples/textedit", directoryHint: .isDirectory),
      FileManager.default.fileExists(atPath: url.path)
    else { return nil }
    return url
  }
}
