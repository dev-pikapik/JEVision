import Foundation
import JEVisionCore
import Testing

@testable import JEVision

/// The Install Block sheet with several blocks: a source that holds more than one, the blocks they
/// need, and the blocks jevision.blocks.json names.
@MainActor
struct BlockInstallListTests {
  private func folder() throws -> URL {
    let url = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: url.appending(path: "blocks"), withIntermediateDirectories: true)
    return url
  }

  /// Writes the package `id` into `shelf`, needing `needs` (id, versions, address).
  @discardableResult
  private func package(
    _ id: String, _ version: String = "1.0.0", in shelf: URL, needs: [(String, String, String?)] = []
  ) throws -> URL {
    let dir = shelf.appending(path: "\(id).jevblock")
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let dependencies = needs.map { id, range, source in
      #"{"id": "\#(id)", "version": "\#(range)"\#(source.map { #", "source": "\#($0)""# } ?? "")}"#
    }
    let name = id.split(separator: ".").last.map { $0.prefix(1).uppercased() + $0.dropFirst() } ?? id
    try """
      {"format": 1, "id": "\(id)", "name": "\(name)", "version": "\(version)", "author": "Acme", "license": "MIT",
       "description": "Does \(name)", "dependencies": [\(dependencies.joined(separator: ", "))]}
      """.write(to: dir.appending(path: "block.json"), atomically: true, encoding: .utf8)
    try """
      {"format": 1, "name": "\(name)", "app": "com.apple.TextEdit", "start": "a", "blocks": {
        "a": {"type": "pressKeys", "keys": "cmd+s"}}}
      """.write(to: dir.appending(path: "flow.json"), atomically: true, encoding: .utf8)
    return dir
  }

  private nonisolated static func issue(_ code: String, _ params: [String: String]) -> JEVisionCore.Issue {
    JEVisionCore.Issue(code: code, severity: .error, params: params.map { IssueParam(name: $0.key, value: $0.value) })
  }

  private func isReview(_ phase: BlockInstallModel.Phase) -> Bool {
    if case .review = phase { true } else { false }
  }

  private func entry(_ model: BlockInstallModel, _ id: String) throws -> BlockInstallEntry {
    try #require(model.entries.first { $0.packageID == id })
  }

  private func ticked(_ model: BlockInstallModel) -> [String] {
    model.entries.filter { model.selection.contains($0.id) }.map(\.packageID)
  }

  /// The live installer, with https addresses read from local folders instead.
  private func installer(_ addresses: [String: URL]) -> BlockInstaller {
    var installer = BlockInstaller.live
    let live = installer.inspect
    installer.inspect = { project, source in
      if source.hasPrefix("https://") {
        guard let folder = addresses[source] else {
          throw BlockPackageError.Failed(issues: [
            Self.issue("block.download.failed", ["url": source, "reason": "no answer"])
          ])
        }
        return try live(project, folder.path)
      }
      return try live(project, source)
    }
    return installer
  }

  @Test func listsTheBlocksOfASourceAndTicksTheNewOnesWithWhatTheyNeed() async throws {
    let (shelf, project) = (try folder(), try folder())
    try package("com.acme.base", in: shelf)
    try package("com.acme.app", in: shelf, needs: [("com.acme.base", "^1.0.0", nil)])
    try package("com.acme.here", in: shelf)
    try package("com.acme.here", in: project.appending(path: "blocks"))
    #expect(BlockInstallModel.source(of: shelf) == shelf.path, "A folder of blocks can be dropped")

    let model = BlockInstallModel(project: project)
    var installed: [String] = []
    model.onInstalled = { installed = $0.map(\.manifest.id) }
    await model.read(shelf.path)
    #expect(model.usesList)
    #expect(Set(model.entries.map(\.packageID)) == ["com.acme.base", "com.acme.app", "com.acme.here"])
    #expect(Set(ticked(model)) == ["com.acme.base", "com.acme.app"], "The block installed already isn't ticked")
    #expect(BlockInstallModel.badge(try entry(model, "com.acme.here")) == "Same Version")
    #expect(model.preview != nil, "One block is shown below the list")

    let (base, app) = (try entry(model, "com.acme.base"), try entry(model, "com.acme.app"))
    #expect(model.requiredBy(base) == ["App"])
    #expect(!model.canToggle(base), "A block another ticked block needs stays ticked")
    #expect(model.notes(for: app).contains { $0.text.contains("Base") }, "It says what goes in with it")
    model.toggle(base)
    #expect(model.selection.contains(base.id))

    model.toggle(app)
    #expect(ticked(model) == ["com.acme.base"])
    model.toggle(base)
    #expect(ticked(model).isEmpty)
    #expect(!model.canInstall)
    model.toggle(app)
    #expect(Set(ticked(model)) == ["com.acme.base", "com.acme.app"], "Ticking a block ticks what it needs")
    #expect(model.plan?.order.count == 2)
    #expect(model.plan?.order.first == base.preview?.folder, "What a block needs goes in first")
    #expect(model.canInstall)

    await model.install()
    guard case .installed(let blocks) = model.phase else {
      Issue.record("Expected the blocks to be installed, got \(model.phase)")
      return
    }
    #expect(blocks.map(\.manifest.id) == ["com.acme.base", "com.acme.app"])
    #expect(installed == ["com.acme.base", "com.acme.app"])
    #expect(Set(listCommunityBlocks(project: project.path).map(\.manifest.id)) == ["com.acme.base", "com.acme.app", "com.acme.here"])
  }

  @Test func fetchesTheBlocksABlockNeedsFromTheAddressesItNames() async throws {
    let (shelf, helpers, project) = (try folder(), try folder(), try folder())
    let address = "https://example.com/helper.zip"
    try package(
      "com.acme.app", in: shelf,
      needs: [("com.acme.helper", "^1.0.0", address), ("com.acme.lost", "*", "https://example.com/lost.zip")])
    try package("com.acme.helper", "1.2.0", in: helpers)
    try package("com.acme.other", in: helpers)

    let model = BlockInstallModel(project: project, installer: installer([address: helpers]))
    await model.read(shelf.appending(path: "com.acme.app.jevblock").path)
    #expect(model.entries.map(\.packageID) == ["com.acme.app", "com.acme.helper"], "Only the block needed is added")
    let (app, helper) = (try entry(model, "com.acme.app"), try entry(model, "com.acme.helper"))
    #expect(helper.neededBy == "App")
    #expect(model.notes(for: helper).first?.text.contains("Needed by App") == true)

    let stops = model.notes(for: app).filter { $0.kind == .problem }.map(\.text)
    #expect(stops.contains { $0.contains("com.acme.lost") && $0.contains("no answer") }, "It says why it couldn't be fetched")
    #expect(!model.canSelect(app), "A block whose needs can't be met can't be installed")
    #expect(!model.selection.contains(app.id))
    #expect(model.canSelect(helper))
  }

  @Test func aNewVersionThatBreaksAnInstalledBlockIsTickedOnlyByHand() async throws {
    let (shelf, project) = (try folder(), try folder())
    let blocks = project.appending(path: "blocks")
    try package("com.acme.base", in: blocks)
    try package("com.acme.user", in: blocks, needs: [("com.acme.base", "^1.0.0", nil)])
    try package("com.acme.base", "2.0.0", in: shelf)
    try package("com.acme.extra", in: shelf)

    let model = BlockInstallModel(project: project)
    await model.read(shelf.path)
    let base = try entry(model, "com.acme.base")
    #expect(ticked(model) == ["com.acme.extra"])
    #expect(model.canSelect(base))
    #expect(model.notes(for: base).contains { $0.kind == .warning && $0.text.contains("com.acme.user") })

    model.toggle(base)
    #expect(model.plan?.problems.map(\.issue.code) == ["block.dependency.breaks"])
    #expect(model.canInstall, "The person was told and ticked it")
    await model.install()
    #expect(listCommunityBlocks(project: project.path).first { $0.manifest.id == "com.acme.base" }?.manifest.version == "2.0.0")
  }

  @Test func restoresTheBlocksJevisionBlocksJsonNames() async throws {
    let (shelf, project) = (try folder(), try folder())
    try package("com.acme.a", in: shelf)
    try package("com.acme.b", "2.0.0", in: shelf)
    try """
      {"format": 1, "sources": {"com.acme.a": "\(shelf.path)", "com.acme.b": "\(shelf.path)", "com.acme.gone": "\(shelf.path)"},
       "versions": {"com.acme.a": "1.0.0", "com.acme.b": "1.5.0"}}
      """.write(to: project.appending(path: "jevision.blocks.json"), atomically: true, encoding: .utf8)

    let store = ProjectStore()
    store.open(project)
    #expect(store.unrestored.count == 3)
    #expect(!store.restoreDeclined)
    store.restoreBlocks()
    let model = try #require(store.installingBlock)
    try await eventuallyAsync("the list is read") { isReview(model.phase) }
    #expect(model.isRestoring)
    #expect(model.entries.map(\.packageID) == ["com.acme.a", "com.acme.b", "com.acme.gone"])
    #expect(ticked(model) == ["com.acme.a"], "Only what the address still has in the saved version")

    let (b, gone) = (try entry(model, "com.acme.b"), try entry(model, "com.acme.gone"))
    #expect(model.notes(for: b).contains { $0.kind == .warning && $0.text.contains("1.5.0") && $0.text.contains("2.0.0") })
    #expect(!model.canSelect(gone))
    #expect(model.notes(for: gone).contains { $0.kind == .problem })
    model.toggle(gone)
    #expect(!model.selection.contains(gone.id))
    model.toggle(b)
    #expect(model.selection.count == 2)

    await model.install()
    #expect(Set(listCommunityBlocks(project: project.path).map(\.manifest.id)) == ["com.acme.a", "com.acme.b"])
    #expect(store.unrestored.map(\.id) == ["com.acme.gone"], "The offer keeps what is left")
    store.restoreDeclined = true
    store.open(project)
    #expect(!store.restoreDeclined, "Not Now lasts until the project opens again")
  }

  @Test func theTestErrorAboutANeededBlockOpensTheSheetOnIt() async throws {
    let shelf = try folder()
    let project = try folder()
    let address = "https://example.com/helper.zip"
    try package("com.acme.extra", in: shelf)
    try package("com.acme.helper", in: shelf)
    let issue = Self.issue(
      "flow.block.dependencyUnmet",
      ["block": "a", "field": "test", "path": "blocks/com.acme.app.jevblock/flow.json", "id": "com.acme.app",
       "by": "com.acme.app", "dependency": "com.acme.helper", "range": "^1.0.0", "source": address])
    #expect(
      IssueText.message(issue)
        == "The block com.acme.app needs the block com.acme.helper ^1.0.0, which isn’t installed. Install it, or remove the step.")
    let need = try #require(BlockNeed(issue))
    #expect(need == BlockNeed(id: "com.acme.helper", source: address))

    let model = BlockInstallModel(project: project)
    model.need = need
    await model.read(shelf.path)
    #expect(model.focused?.packageID == "com.acme.helper", "The block the test needs is the one shown")
    #expect(model.selection.contains(try entry(model, "com.acme.helper").id))
    #expect(model.needMismatch == nil)
    model.need = BlockNeed(id: "com.acme.other")
    #expect(model.needMismatch == .otherBlock(wanted: "com.acme.other"))
  }

  @Test func namesTheProblemsOfTheNewCodesInWords() {
    let issue = Self.issue
    let missing = IssueText.message(
      issue("block.dependency.missing", ["id": "com.acme.b", "range": "^1.0.0", "by": "com.acme.a", "source": "https://x.io/b.zip"]))
    #expect(missing == "com.acme.a needs the block com.acme.b ^1.0.0, which isn’t installed. Install it from https://x.io/b.zip.")
    let any = IssueText.message(issue("block.dependency.missing", ["id": "com.acme.b", "range": "*", "by": "com.acme.a"]))
    #expect(any == "com.acme.a needs the block com.acme.b, which isn’t installed. Install it first.")
    let breaks = IssueText.message(
      issue("block.dependency.breaks", ["id": "com.acme.u", "dependency": "com.acme.b", "range": "^1.0.0", "found": "2.0.0"]))
    #expect(breaks.contains("com.acme.b 2.0.0"))
    for code in ["block.dependency.cycle", "block.set.duplicate", "block.set.tooMany", "block.update.missing", "block.copy.nested"] {
      #expect(!IssueText.message(issue(code, [:])).contains(code), "\(code) has words")
    }
  }
}
