import Foundation
import JEVisionCore

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
  let keywords: String
  /// Someone marked the test as flaky in its details.
  let markedFlaky: Bool
  /// What its card says: type, priority, status and author, to choose tests by.
  let card: JEVisionCore.TestCard
  /// What the search looks in besides the file name: the folder and `keywords`, put together
  /// once rather than on every letter typed.
  let searchKeywords: String

  init(
    id: String, name: String, url: URL, kind: Kind, children: [ProjectItem], testCount: Int, modified: Date?,
    keywords: String = "", markedFlaky: Bool = false, card: JEVisionCore.TestCard = JEVisionCore.TestCard()
  ) {
    self.id = id
    self.name = name
    self.url = url
    self.kind = kind
    self.children = children
    self.testCount = testCount
    self.modified = modified
    self.keywords = keywords
    self.markedFlaky = markedFlaky
    self.card = card
    let folder = id.split(separator: "/").dropLast().joined(separator: "/")
    searchKeywords = folder + " " + keywords
  }

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

extension [ProjectItem] {
  /// The tests, at any depth, whose cards `filter` chooses; every one when it asks nothing.
  /// `flaky` holds the tests found flaky from their runs, which count like the mark in their
  /// details.
  func chosen(by filter: TestFilter, flaky: Set<String>) -> [ProjectItem] {
    let tests = flatMap(\.tests)
    guard !testFilterIsEmpty(filter: filter) else { return tests }
    let cards = tests.map { test in
      var card = test.card
      if flaky.contains(test.id) { card.flaky = true }
      return card
    }
    return filterTests(filter: filter, cards: cards).map { tests[Int($0)] }
  }

  /// Only the tests with these ids and the folders that hold them.
  func keeping(_ ids: Set<String>) -> [ProjectItem] {
    compactMap { item in
      guard item.kind == .folder else { return ids.contains(item.id) ? item : nil }
      let children = item.children.keeping(ids)
      guard !children.isEmpty else { return nil }
      return ProjectItem(
        id: item.id, name: item.name, url: item.url, kind: .folder, children: children,
        testCount: children.reduce(0) { $0 + $1.testCount }, modified: item.modified)
    }
  }
}
