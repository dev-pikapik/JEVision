import Foundation
import JEVisionCore
import Testing

@testable import JEVision

/// A new folder in the temporary folder, as bookmarks and the core see it: the temporary folder
/// is behind a link.
private func temporaryHome() throws -> URL {
  let folder = FileManager.default.temporaryDirectory.appending(path: "jevision-new-project-\(UUID().uuidString)")
  try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
  return URL(filePath: try #require(folder.resourceValues(forKeys: [.canonicalPathKey]).canonicalPath), directoryHint: .isDirectory)
}

/// The example that ships with the app, from this repository.
private let example = URL(filePath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
  .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
  .appending(path: "examples/textedit", directoryHint: .isDirectory)

/// A local port that takes connections and never answers: a copy from it waits until it is
/// cancelled, however busy the machine is.
private func silentServer() throws -> (port: UInt16, socket: Int32) {
  let socket = Darwin.socket(AF_INET, SOCK_STREAM, 0)
  var address = sockaddr_in()
  address.sin_family = sa_family_t(AF_INET)
  address.sin_addr.s_addr = inet_addr("127.0.0.1")
  var size = socklen_t(MemoryLayout<sockaddr_in>.size)
  let listening = withUnsafeMutablePointer(to: &address) {
    $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
      bind(socket, $0, size) == 0 && listen(socket, 4) == 0 && getsockname(socket, $0, &size) == 0
    }
  }
  try #require(socket >= 0 && listening)
  return (UInt16(bigEndian: address.sin_port), socket)
}

/// Waits up to two seconds for `condition`, checking it on the main actor between waits.
@MainActor
private func waitUntil(_ condition: () -> Bool) async {
  for _ in 0..<200 where !condition() { try? await Task.sleep(for: .milliseconds(10)) }
}

@MainActor
final class NewProjectModelTests {
  let home: URL

  init() throws { home = try temporaryHome() }
  deinit { try? FileManager.default.removeItem(at: home) }

  private var documents: URL { home.appending(path: "Documents", directoryHint: .isDirectory) }

  @Test func checksTheNameAsItIsTyped() throws {
    let model = NewProjectModel(.new, home: home, example: example)
    #expect(model.name.isEmpty)
    #expect(model.nameProblem != nil, "An empty name won't do")
    #expect(!model.canCreate)
    #expect(model.target == nil)

    model.name = "Shop"
    #expect(model.nameProblem == nil)
    #expect(model.canCreate)
    #expect(model.target == documents.appending(path: "JEVision/Shop").path)

    model.name = "Shop/Cart"
    #expect(model.nameProblem != nil)
    #expect(!model.canCreate)

    let taken = documents.appending(path: "JEVision/Taken")
    try FileManager.default.createDirectory(at: taken, withIntermediateDirectories: true)
    try Data("{}".utf8).write(to: taken.appending(path: "a.flow.json"))
    model.name = "Taken"
    #expect(model.nameProblem != nil, "A folder with something in it is taken")
  }

  @Test func offersTheFoldersOfTheHomeAndOneChosen() throws {
    #expect(NewProjectModel(.new, home: home).locations == [documents.appending(path: "JEVision", directoryHint: .isDirectory)])

    try FileManager.default.createDirectory(at: documents.appending(path: "GitHub"), withIntermediateDirectories: true)
    let model = NewProjectModel(.new, home: home)
    #expect(model.locations.map(\.lastPathComponent) == ["JEVision", "GitHub"])
    #expect(model.location == model.locations[0])
    #expect(model.title(of: model.locations[1]) == String(localized: "Documents › \("GitHub")"))

    let other = home.appending(path: "Work", directoryHint: .isDirectory)
    model.choose(location: other)
    model.choose(location: other)
    #expect(model.locations.count == 3, "A folder chosen twice is offered once")
    #expect(model.location == other)
    #expect(model.title(of: other) == other.path)
    model.name = "Shop"
    #expect(model.target == other.appending(path: "Shop").path)
  }

  @Test func mapsTheStarterToTheCore() throws {
    let model = NewProjectModel(.new, home: home, example: example)
    #expect(model.starter == .empty)
    #expect(model.coreStarter == .empty)

    model.choose(.example)
    #expect(model.coreStarter == .copyOf(folder: example.path(percentEncoded: false)))
    #expect(model.name == ExampleProject.name, "The example names the project until the person does")

    let source = home.appending(path: "Team Template", directoryHint: .isDirectory)
    model.copy(of: source)
    #expect(model.starter == .copy)
    #expect(model.coreStarter == .copyOf(folder: source.path(percentEncoded: false)))
    #expect(model.name == "Team Template")

    model.name = "Mine"
    model.choose(.example)
    #expect(model.name == "Mine", "A name the person typed stays")

    let without = NewProjectModel(.new, home: home, example: nil)
    without.choose(.example)
    #expect(without.starter == .empty, "There is no example to copy")
  }

  @Test func anExampleRequestStartsWithTheExample() async throws {
    let model = NewProjectModel(.example, home: home, example: example)
    #expect(model.mode == .new)
    #expect(model.starter == .example)
    #expect(model.name == ExampleProject.name)
    #expect(NewProjectModel(.example, home: home, example: nil).starter == .empty)

    let before = try FileManager.default.contentsOfDirectory(atPath: example.path).sorted()
    let made = try #require(await model.create())
    #expect(model.firstTest(of: made) == ExampleProject.firstTest)
    #expect(FileManager.default.fileExists(atPath: URL(filePath: made.path).appending(path: ExampleProject.firstTest).path))
    #expect(try FileManager.default.contentsOfDirectory(atPath: example.path).sorted() == before, "The example in the app stays as it was")

    let again = NewProjectModel(.example, home: home, example: example)
    #expect(again.name == "\(ExampleProject.name) 2", "A second copy takes the next free name")
  }

  @Test func makesAnEmptyProjectAndSaysWhereItsHistoryIsKept() async throws {
    let model = NewProjectModel(.new, home: home, example: example)
    await model.lookAtHistory()
    #expect(model.history == (gitInstalled() ? .own : .unavailable))
    model.name = "Shop"
    let made = try #require(await model.create())
    #expect(made.path == documents.appending(path: "JEVision/Shop").path)
    #expect(made.git == gitInstalled())
    #expect(model.firstTest(of: made) == nil)
    #expect(model.failure == nil)
  }

  @Test func takesTheRepositoryAndItsNameFromTheClipboard() {
    let model = NewProjectModel(.clone, home: home, copied: "  https://github.com/acme/shop-tests.git\n")
    #expect(model.mode == .clone)
    #expect(model.address == "https://github.com/acme/shop-tests.git")
    #expect(model.name == "shop-tests")

    let sentence = NewProjectModel(.clone, home: home, copied: "Buy milk and bread")
    #expect(sentence.address.isEmpty, "Other text isn't offered as an address")
    #expect(sentence.name.isEmpty)
    #expect(!sentence.canCreate)
    sentence.paste("git@github.com:acme/bank.git ")
    #expect(sentence.address == "git@github.com:acme/bank.git")
    #expect(sentence.name == "bank")

    sentence.name = "Bank Tests"
    sentence.address = "https://github.com/acme/payments"
    #expect(sentence.name == "Bank Tests", "A name the person typed stays")

    let switched = NewProjectModel(.new, home: home)
    switched.mode = .clone
    switched.address = "https://github.com/acme/shop"
    #expect(switched.name == "shop")
    #expect(NewProjectModel(.new, home: home, copied: "https://github.com/acme/shop").address.isEmpty)
  }

  @Test(.enabled(if: gitInstalled())) func clonesARepositoryAndSaysWhyNot() async throws {
    let source = URL(filePath: try createProject(parent: home.path, name: "Source", starter: .empty).path)
    let model = NewProjectModel(.clone, home: home)
    model.address = "file://\(source.path)"
    #expect(model.name == "Source")
    let copy = try #require(await model.clone())
    #expect(copy.path == documents.appending(path: "JEVision/Source").path)
    #expect(FileManager.default.fileExists(atPath: copy.appending(path: ".git").path))
    #expect(model.failure == nil)
    #expect(!model.isWorking)

    let missing = NewProjectModel(.clone, home: home)
    missing.address = "file://\(home.path)/Nothing.git"
    #expect(await missing.clone() == nil)
    let failure = try #require(missing.failure)
    #expect(failure.field == .address)
    #expect(!failure.message.isEmpty)
    #expect(!FileManager.default.fileExists(atPath: documents.appending(path: "JEVision/Nothing").path))
  }

  @Test(.enabled(if: gitInstalled())) func cancellingTheCopyStopsItAndLeavesNothing() async throws {
    let server = try silentServer()
    defer { close(server.socket) }
    let model = NewProjectModel(.clone, home: home)
    model.address = "http://127.0.0.1:\(server.port)/Source.git"
    let copying = Task { await model.clone() }
    await waitUntil { model.isWorking }
    try #require(model.isWorking, "The copy is under way: \(model.failure?.message ?? "")")
    model.cancelClone()
    #expect(await copying.value == nil)
    #expect(model.failure == nil, "Cancelling isn't a failure")
    #expect(!model.isWorking)
    #expect(!FileManager.default.fileExists(atPath: documents.appending(path: "JEVision/Source").path))
  }
}

@MainActor
final class ProjectGuardTests {
  let suite = "jevision.tests.\(UUID().uuidString)"
  let defaults: UserDefaults
  let home: URL
  /// What the fake run does: whether it still runs, and how often it was asked about and stopped.
  var running = false
  var asked: [String] = []
  var stopped = 0

  init() throws {
    defaults = try #require(UserDefaults(suiteName: suite))
    home = try temporaryHome()
  }

  deinit {
    UserDefaults().removePersistentDomain(forName: suite)
    try? FileManager.default.removeItem(at: home)
  }

  private func project(_ name: String) throws -> URL {
    let url = home.appending(path: name, directoryHint: .isDirectory)
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    try Data("{}".utf8).write(to: url.appending(path: "jevision.project.json"))
    try Data("{}".utf8).write(to: url.appending(path: "Sign In.flow.json"))
    return url
  }

  /// A store whose test “Sign In” runs while `running` is set; stopping it ends it a moment later,
  /// as a real run ends once the core has stopped it.
  private func makeStore(agrees: Bool) -> ProjectStore {
    let store = ProjectStore(defaults: defaults)
    store.activeWork = { [unowned self] in
      guard running else { return nil }
      return ProjectWork(kind: .run, test: "Sign In") { [unowned self] in
        stopped += 1
        Task { self.running = false }
      }
    }
    store.asksToStop = { [unowned self] work in
      asked.append(work.test)
      return agrees
    }
    return store
  }

  @Test func leavesAtOnceWhenNothingRuns() async throws {
    let (shop, bank) = (try project("Shop"), try project("Bank"))
    let store = makeStore(agrees: false)
    store.openProject(shop)
    store.openProject(bank)
    #expect(store.rootURL?.path == bank.path)
    #expect(await store.readyToLeave())
    store.close()
    #expect(store.rootURL == nil)
    #expect(asked.isEmpty)
  }

  @Test func staysWhenThePersonWantsTheRunToGoOn() async throws {
    let (shop, bank) = (try project("Shop"), try project("Bank"))
    let store = makeStore(agrees: false)
    store.openProject(shop)
    running = true

    store.openProject(bank)
    await waitUntil { asked.count == 1 }
    store.close()
    await waitUntil { asked.count == 2 }
    #expect(await store.readyToLeave() == false)
    #expect(asked == ["Sign In", "Sign In", "Sign In"])
    #expect(store.rootURL?.path == shop.path)
    #expect(stopped == 0)
    #expect(running)

    store.openProject(shop)
    #expect(asked.count == 3, "Opening the open project again leaves nothing")
  }

  @Test func stopsTheRunAndThenGoesOn() async throws {
    let (shop, bank) = (try project("Shop"), try project("Bank"))
    let store = makeStore(agrees: true)
    store.openProject(shop)
    running = true

    store.openProject(bank, test: "Sign In.flow.json")
    #expect(store.rootURL?.path == shop.path, "Nothing changes before the run has stopped")
    await waitUntil { store.rootURL?.path == bank.path }
    #expect(store.rootURL?.path == bank.path)
    #expect(store.selection == "Sign In.flow.json")
    #expect(asked == ["Sign In"])
    #expect(stopped == 1)
    #expect(!running)

    running = true
    store.close()
    await waitUntil { store.rootURL == nil }
    #expect(store.rootURL == nil)
    #expect(stopped == 2)
  }
}

@MainActor
final class MyTestsProjectTests {
  let suite = "jevision.tests.\(UUID().uuidString)"
  let defaults: UserDefaults
  let home: URL

  init() throws {
    defaults = try #require(UserDefaults(suiteName: suite))
    home = try temporaryHome()
  }

  deinit {
    UserDefaults().removePersistentDomain(forName: suite)
    try? FileManager.default.removeItem(at: home)
  }

  private var folder: URL {
    home.appending(path: "Documents/JEVision/\(String(localized: "My Tests"))", directoryHint: .isDirectory)
  }

  @Test func makesMyTestsOnceAndOpensItAfterwards() throws {
    let store = ProjectStore(defaults: defaults)
    try store.openMyTests(home: home)
    #expect(store.rootURL?.path == folder.path)
    #expect(FileManager.default.fileExists(atPath: folder.appending(path: "jevision.project.json").path))
    #expect(store.recent.map(\.path) == [folder.path])

    try Data("{}".utf8).write(to: folder.appending(path: "Mine.flow.json"))
    store.close()
    try store.openMyTests(home: home)
    #expect(store.rootURL?.path == folder.path)
    #expect(FileManager.default.fileExists(atPath: folder.appending(path: "Mine.flow.json").path), "Made once, then opened")
  }

  @Test func isKnownByItsFolderWhateverTheLanguage() throws {
    let store = ProjectStore(defaults: defaults)
    try store.openMyTests(home: home, name: "My Tests")
    let made = try #require(store.rootURL)
    store.close()

    // The app's language is Russian now: the name it would give is another.
    try store.openMyTests(home: home, name: "Мои тесты")
    #expect(store.rootURL?.path == made.path, "The one made before opens")
    #expect(!FileManager.default.fileExists(atPath: made.deletingLastPathComponent().appending(path: "Мои тесты").path))
    #expect(store.recent.map(\.path) == [made.path])
    #expect(store.myTestsName == "My Tests")

    store.close()
    let moved = home.appending(path: "Documents/Mine", directoryHint: .isDirectory)
    try FileManager.default.moveItem(at: made, to: moved)
    try store.openMyTests(home: home, name: "Мои тесты")
    #expect(store.rootURL?.path == moved.path, "A folder moved on the disk is followed")
    #expect(store.recent.map(\.path) == [moved.path])

    store.close()
    try FileManager.default.removeItem(at: moved)
    #expect(store.myTestsName == String(localized: "My Tests"), "Gone, it gets the name of the app's language")
    try store.openMyTests(home: home, name: "Мои тесты")
    #expect(store.rootURL?.lastPathComponent == "Мои тесты", "Made again")
  }

  @Test func aFileInTheWayIsSaidAndNothingOpens() throws {
    try FileManager.default.createDirectory(at: folder.deletingLastPathComponent(), withIntermediateDirectories: true)
    try Data("notes".utf8).write(to: URL(filePath: folder.path))
    let store = ProjectStore(defaults: defaults)
    #expect(throws: ProjectFolderError.self) { try store.openMyTests(home: home) }
    #expect(store.rootURL == nil)
  }

  @Test func theProjectOpenBeforeStaysOpen() throws {
    let old = home.appending(path: "Documents/JEVision Tests", directoryHint: .isDirectory)
    try FileManager.default.createDirectory(at: old, withIntermediateDirectories: true)
    try Data("{}".utf8).write(to: old.appending(path: "Old.flow.json"))
    ProjectStore(defaults: defaults).openProject(old)

    let store = ProjectStore(defaults: defaults)
    #expect(store.rootURL?.path == old.path, "The tests folder of earlier versions opens as it did")
    try store.openDemo()
    #expect(store.rootURL?.path == old.path, "The demo goes in the open project")
    #expect(!FileManager.default.fileExists(atPath: folder.path))
  }
}
