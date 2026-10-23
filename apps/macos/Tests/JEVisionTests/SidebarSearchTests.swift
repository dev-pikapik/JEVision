import Foundation
import Synchronization
import Testing

@testable import JEVision

/// The sidebar's search keeps up with typing: the tests are looked through once typing pauses,
/// off the main thread, and only what the latest text finds is shown.
@MainActor struct SidebarSearchTests {
  /// Counts the searches made, from whichever thread makes them.
  final class Searches: Sendable {
    let made = Mutex(0)
    /// Seconds the search for a text takes.
    let delays: [String: Double]

    init(delays: [String: Double] = [:]) {
      self.delays = delays
    }

    func rank(_ items: [ProjectItem], _ text: String) -> [ProjectItem] {
      made.withLock { $0 += 1 }
      if let delay = delays[text] { Thread.sleep(forTimeInterval: delay) }
      return items.filter { $0.name.localizedCaseInsensitiveContains(text) }
    }
  }

  private let items = ["Add coupon", "Checkout", "Checkout coupon", "Dark mode"].map { name in
    ProjectItem(
      id: name + ".flow.json", name: name, url: URL(filePath: "/tmp/\(name).flow.json"), kind: .test, children: [],
      testCount: 1, modified: nil)
  }

  private func waitUntil(_ done: () -> Bool) async throws {
    let deadline = ContinuousClock.now + .seconds(20)
    while !done() {
      try #require(ContinuousClock.now < deadline, "Gave up waiting for the search")
      try await Task.sleep(for: .milliseconds(5))
    }
  }

  @Test func typingQuicklySearchesOnceForTheLastText() async throws {
    let searches = Searches()
    let search = SidebarSearch(rank: searches.rank)
    search.search(in: items)
    let query = "checkout coupon"
    for end in 1...query.count {
      search.text = String(query.prefix(end))
      await Task.yield()
    }
    #expect(search.text == query, "What is typed shows at once")
    try await waitUntil { search.query == query }
    #expect(search.found.map(\.name) == ["Checkout coupon"])
    #expect(searches.made.withLock { $0 } <= 2, "\(query.count) letters typed in a row are looked up once or twice")
  }

  @Test func anOlderSearchNeverReplacesANewerOne() async throws {
    // The search for "c" is still going when "cou" is typed, and ends after it.
    let searches = Searches(delays: ["c": 0.6])
    let search = SidebarSearch(rank: searches.rank)
    search.search(in: items)
    search.text = "c"
    try await waitUntil { searches.made.withLock { $0 } == 1 }
    search.text = "cou"
    try await waitUntil { search.query == "cou" }
    try await Task.sleep(for: .milliseconds(800))
    #expect(search.query == "cou")
    #expect(search.found.map(\.name) == ["Add coupon", "Checkout coupon"])
  }

  @Test func clearingTheTextShowsTheFoldersAtOnce() async throws {
    let search = SidebarSearch(rank: Searches().rank)
    search.search(in: items)
    search.text = "dark"
    try await waitUntil { search.isSearching }
    search.text = ""
    #expect(!search.isSearching)
    #expect(search.found.isEmpty)
    search.text = "   "
    #expect(!search.isSearching, "Spaces alone search nothing")
  }

  @Test func newTestsAreSearchedAgainWithoutWaitingForTyping() async throws {
    let search = SidebarSearch(rank: Searches().rank)
    search.search(in: Array(items.prefix(2)))
    search.text = "coupon"
    try await waitUntil { search.isSearching }
    #expect(search.found.map(\.name) == ["Add coupon"])
    search.search(in: items)
    try await waitUntil { search.found.count == 2 }
    #expect(search.found.map(\.name) == ["Add coupon", "Checkout coupon"])
  }
}
