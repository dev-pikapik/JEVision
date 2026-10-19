import Foundation

/// A test file or a folder that holds tests, as shown in the sidebar.
struct ProjectItem: Identifiable, Hashable, Sendable {
  enum Kind: Sendable { case folder, test }

  /// Path relative to the project folder, stable across rescans.
  let id: String
  let name: String
  let url: URL
  let kind: Kind
  let children: [ProjectItem]
  let testCount: Int
  let modified: Date?
  /// What a test is found by besides its file name: its name, description, details and block texts.
  var keywords = ""
  /// Someone marked the test as flaky in its details.
  var markedFlaky = false

  /// Children for an outline: `nil` marks a leaf.
  var outlineChildren: [ProjectItem]? { kind == .folder ? children : nil }

  /// The tests in the item, at any depth, in sidebar order; the item itself for a test.
  var tests: [ProjectItem] { kind == .test ? [self] : children.flatMap(\.tests) }

  /// Where the item lives inside the project, e.g. "Smoke/Login"; empty at the top level.
  var parentPath: String {
    let parts = id.split(separator: "/").dropLast()
    return parts.joined(separator: "/")
  }
}

extension [ProjectItem] {
  func item(withID id: String) -> ProjectItem? {
    for item in self {
      if item.id == id { return item }
      if let found = item.children.item(withID: id) { return found }
    }
    return nil
  }
}
