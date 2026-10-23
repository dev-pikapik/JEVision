import Foundation
import JEVisionCore
import SwiftUI
import Testing

@testable import JEVision

@MainActor
final class RecentProjectsTests {
  let suite = "jevision.tests.\(UUID().uuidString)"
  let defaults: UserDefaults
  let folder: URL
  /// A minute passes every time a project is opened, so the list's order never depends on a tie.
  var time = 1_800_000_000.0

  init() throws {
    defaults = try #require(UserDefaults(suiteName: suite))
    let folder = FileManager.default.temporaryDirectory.appending(path: "jevision-recent-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    // The temporary folder is behind a link: compare paths as bookmarks lead to them.
    self.folder = URL(filePath: try #require(folder.resourceValues(forKeys: [.canonicalPathKey]).canonicalPath))
  }

  deinit {
    UserDefaults().removePersistentDomain(forName: suite)
    try? FileManager.default.removeItem(at: folder)
  }

  private func makeStore() -> ProjectStore {
    ProjectStore(defaults: defaults) { [unowned self] in
      time += 60
      return Date(timeIntervalSince1970: time)
    }
  }

  /// A project folder with a settings file and one test, `Login/Sign In.flow.json`.
  private func project(_ name: String) throws -> URL {
    let url = folder.appending(path: name, directoryHint: .isDirectory)
    try FileManager.default.createDirectory(at: url.appending(path: "Login"), withIntermediateDirectories: true)
    try Data("{}".utf8).write(to: url.appending(path: "jevision.project.json"))
    try Data("{}".utf8).write(to: url.appending(path: "Login/Sign In.flow.json"))
    return url
  }

  @Test func keepsTheListAndReopensTheLastProject() throws {
    let shop = try project("Shop")
    let store = makeStore()
    store.openProject(shop)
    #expect(store.recent.map(\.path) == [shop.path])
    #expect(store.recent.first?.name == "Shop")

    let next = makeStore()
    #expect(next.recent.map(\.path) == [shop.path])
    #expect(next.rootURL?.path == shop.path)
  }

  @Test func movesTheProjectOfEarlierVersionsIntoTheList() throws {
    let shop = try project("Shop")
    defaults.set(try shop.bookmarkData(), forKey: ProjectStore.Key.bookmark)
    defaults.set("Shop", forKey: ProjectStore.Key.name)
    let store = makeStore()
    #expect(store.recent.map(\.path) == [shop.path])
    #expect(store.rootURL?.path == shop.path)
    #expect(defaults.object(forKey: ProjectStore.Key.bookmark) == nil)
    #expect(defaults.object(forKey: ProjectStore.Key.name) == nil)
  }

  @Test func movesAProjectOfEarlierVersionsThatIsGone() throws {
    let shop = try project("Shop")
    defaults.set(try shop.bookmarkData(), forKey: ProjectStore.Key.bookmark)
    defaults.set("Shop Tests", forKey: ProjectStore.Key.name)
    try FileManager.default.removeItem(at: shop)
    let store = makeStore()
    #expect(store.status == .closed)
    #expect(store.recent.map(\.path) == [shop.path], "Its card says the folder can't be found")
    #expect(store.recent.first?.name == "Shop Tests")
  }

  @Test func showsTheProjectsWhenTheLastFolderIsGone() async throws {
    let shop = try project("Shop")
    makeStore().openProject(shop)
    try FileManager.default.removeItem(at: shop)

    let store = makeStore()
    #expect(store.status == .closed)
    await store.summaryTask?.value
    #expect(store.summaries[shop.path]?.exists == false)
    #expect(store.openableRecent.isEmpty)
  }

  @Test func openingPutsTheProjectFirst() throws {
    let (shop, blog) = (try project("Shop"), try project("Blog"))
    let store = makeStore()
    store.openProject(shop)
    store.openProject(blog)
    #expect(store.recent.map(\.path) == [blog.path, shop.path])
    #expect(defaults.string(forKey: ProjectStore.Key.current) == blog.path)

    store.openRecent(shop.path)
    #expect(store.rootURL?.path == shop.path)
    #expect(store.recent.map(\.path) == [shop.path, blog.path])
  }

  @Test func pinsRemovesAndClears() throws {
    let (shop, blog, docs) = (try project("Shop"), try project("Blog"), try project("Docs"))
    let store = makeStore()
    for url in [shop, blog, docs] { store.openProject(url) }
    store.close()
    store.setPinned(shop.path, true)
    #expect(store.recent.map(\.path) == [shop.path, docs.path, blog.path])
    #expect(store.recent.first?.pinned == true)

    store.removeRecent(docs.path)
    #expect(store.recent.map(\.path) == [shop.path, blog.path])
    #expect(FileManager.default.fileExists(atPath: docs.path), "Only the list forgets it")

    store.clearRecent()
    #expect(store.recent.map(\.path) == [shop.path], "Clear Menu keeps the pinned projects")
    #expect(makeStore().recent == store.recent)
  }

  @Test func menusTellProjectsOfTheSameNameApart() throws {
    let (shop, blog) = (try project("Shop"), try project("Blog"))
    let other = folder.appending(path: "Clients/Shop", directoryHint: .isDirectory)
    try FileManager.default.createDirectory(at: other, withIntermediateDirectories: true)
    let store = makeStore()
    for url in [shop, other, blog] { store.openProject(url) }
    let entry = { (url: URL) in try #require(store.recent.first { $0.path == url.path }) }
    #expect(store.menuTitle(try entry(blog)) == "Blog")
    #expect(store.menuTitle(try entry(shop)) == "Shop — \(ProjectStore.shortPath(folder.path))")
    #expect(store.menuTitle(try entry(other)) == "Shop — \(ProjectStore.shortPath(other.deletingLastPathComponent().path))")
  }

  @Test func followsAFolderThatMoved() throws {
    let shop = try project("Shop")
    let store = makeStore()
    store.openProject(shop)
    store.setPinned(shop.path, true)
    store.close()
    let moved = folder.appending(path: "Shop 2", directoryHint: .isDirectory)
    try FileManager.default.moveItem(at: shop, to: moved)

    store.openRecent(shop.path)
    #expect(store.rootURL?.path == moved.path)
    #expect(store.recent.map(\.path) == [moved.path])
    #expect(store.recent.first?.pinned == true)

    let other = folder.appending(path: "Shop 3", directoryHint: .isDirectory)
    try FileManager.default.moveItem(at: moved, to: other)
    store.relocate(moved.path, to: other)
    #expect(store.recent.map(\.path) == [other.path])
    #expect(store.recent.first?.name == "Shop 3")
    #expect(store.recent.first?.pinned == true)
  }

  @Test func reopensAFolderThatMovedSinceLastTime() throws {
    let shop = try project("Shop")
    makeStore().openProject(shop)
    let moved = folder.appending(path: "Shop 2", directoryHint: .isDirectory)
    try FileManager.default.moveItem(at: shop, to: moved)

    #expect(makeStore().rootURL?.path == moved.path)
    let next = makeStore()
    #expect(next.rootURL?.path == moved.path)
    #expect(next.recent.map(\.path) == [moved.path])
  }

  @Test func closingShowsTheProjectsAndLetsTheFolderGo() throws {
    let shop = try project("Shop")
    let store = makeStore()
    store.openProject(shop)
    store.close()
    // Opened again from the list, the folder is used through its bookmark.
    store.openRecent(shop.path)
    #expect(store.accessedURL != nil)

    store.close()
    #expect(store.status == .closed)
    #expect(store.rootURL == nil)
    #expect(store.items.isEmpty)
    #expect(store.accessedURL == nil)
    #expect(store.recent.map(\.path) == [shop.path])
    #expect(makeStore().status == .closed, "The next launch shows the projects too")
  }

  @Test func opensWhatTheFinderHandsOver() throws {
    let shop = try project("Shop")
    try Data("notes".utf8).write(to: shop.appending(path: "notes.txt"))
    let store = makeStore()

    #expect(store.openItem(shop.appending(path: "Login/Sign In.flow.json")))
    #expect(store.rootURL?.path == shop.path)
    #expect(store.selection == "Login/Sign In.flow.json")
    store.close()

    #expect(store.openItem(shop.appending(path: "jevision.project.json")))
    #expect(store.rootURL?.path == shop.path)
    store.close()

    #expect(store.openItem(shop.appending(path: "Login", directoryHint: .isDirectory)))
    #expect(store.rootURL?.path == shop.path, "A folder inside opens its project")
    store.close()

    #expect(!store.openItem(shop.appending(path: "notes.txt")))
    #expect(store.rootURL == nil)
    #expect(store.recent.map(\.path) == [shop.path])
  }

  @Test func arrowsMoveThroughTheCards() {
    #expect(ProjectsHomeView.columns(for: 250) == 1)
    #expect(ProjectsHomeView.columns(for: 515) == 1)
    #expect(ProjectsHomeView.columns(for: 516) == 2)

    // Pinned a b c and recent d e, two a row: [a b] [c] [d e].
    let sections = [["a", "b", "c"], ["d", "e"]]
    func step(_ from: String?, _ direction: MoveCommandDirection) -> String? {
      ProjectsHomeView.step(from: from, direction, in: sections, columns: 2)
    }
    #expect(step(nil, .down) == "a")
    #expect(step("a", .left) == "a")
    #expect(step("b", .right) == "c")
    #expect(step("b", .down) == "c")
    #expect(step("c", .down) == "d")
    #expect(step("e", .up) == "c")
    #expect(step("e", .down) == "e")
  }
}
