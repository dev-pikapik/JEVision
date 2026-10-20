import Foundation
import JEVisionCore
import Testing

@testable import JEVision

@MainActor
struct CommunityBlocksTests {
  private static let block = """
    {"format": 1, "name": "Sign in", "icon": "person", "app": "com.apple.TextEdit", "start": "a", "blocks": {
      "a": {"type": "type", "text": "${input.email}", "next": "b"},
      "b": {"type": "command", "command": "say hello"}}}
    """
  private static let blockFile = "blocks/Sign in.block.json"

  private func folder() throws -> URL {
    let url = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: url.appending(path: "blocks"), withIntermediateDirectories: true)
    return url
  }

  /// A project with its own block, and a project to install it in.
  private func projects() throws -> (mine: URL, theirs: URL) {
    let mine = try folder()
    try Self.block.write(to: mine.appending(path: Self.blockFile), atomically: true, encoding: .utf8)
    return (mine, try folder())
  }

  private func share(_ version: String) -> BlockShare {
    BlockShare(
      id: "com.anna.sign-in", name: "", description: "Types an email", author: "Anna", version: version,
      license: "MIT", inputs: [BlockPackageInput(name: "email", label: "Email", defaultValue: nil)])
  }

  private func zip(_ version: String, from mine: URL) throws -> String {
    let file = mine.appending(path: "sign-in-\(version).zip").path
    _ = try exportBlockPackage(
      project: mine.path, blockFile: Self.blockFile, share: share(version), destination: file, asZip: true)
    return file
  }

  private func isReview(_ phase: BlockInstallModel.Phase) -> Bool {
    if case .review = phase { true } else { false }
  }

  @Test func readingABlockShowsWhatItDoesAndInstallsNothing() async throws {
    let (mine, theirs) = try projects()
    let model = BlockInstallModel(project: theirs)
    await model.read(try zip("1.0.0", from: mine))

    #expect(isReview(model.phase))
    #expect(model.errors.isEmpty)
    #expect(model.canInstall)
    #expect(model.conflict == nil)
    #expect(model.steps.map(\.number) == [1, 2])
    #expect(model.steps.map(\.id) == ["a", "b"])
    #expect(model.steps[1].summary.contains("say hello"))
    #expect(model.riskGroups.map(\.risk) == [.script, .keyboard], "The most worrying kind comes first")
    #expect(model.riskGroups[0].steps.first?.detail == "say hello")
    #expect(model.riskGroups.allSatisfy { $0.isSerious })
    #expect(listCommunityBlocks(project: theirs.path).isEmpty, "Reading doesn't install")
  }

  @Test func installsTheBlockIntoTheLibraryAsCommunity() async throws {
    let (mine, theirs) = try projects()
    let model = BlockInstallModel(project: theirs)
    var installed: String?
    model.onInstalled = { installed = $0.first?.manifest.id }
    await model.read(try zip("1.0.0", from: mine))
    let staged = try #require(model.preview).folder
    await model.install()

    #expect(installed == "com.anna.sign-in")
    guard case .installed(let blocks) = model.phase else {
      Issue.record("Expected the block to be installed, got \(model.phase)")
      return
    }
    #expect(blocks.map(\.manifest.version) == ["1.0.0"])
    #expect(!FileManager.default.fileExists(atPath: staged), "What was unpacked for reading is gone")

    let entry = try #require(ProjectLibrary.communityEntries(theirs).first)
    #expect(entry.name == "Sign in")
    #expect(entry.inputs == ["email"])
    #expect(entry.package?.author == "Anna")
    #expect(entry.package?.version == "1.0.0")
    #expect(entry.package?.source == nil, "A file has no address to update from")
    #expect(entry.url?.lastPathComponent == "flow.json")
    #expect(PaletteChoice.library(entry).groupTitle == "Community")
    #expect(PaletteChoice.library(entry).tagline.contains("Anna"))
    #expect(ProjectLibrary.read(theirs).last == entry, "Community blocks come after the project's own")
  }

  @Test func theLabelsAndDefaultsOfInputsReachTheLibrary() async throws {
    let (mine, theirs) = try projects()
    var share = share("1.0.0")
    share.inputs = [BlockPackageInput(name: "email", label: "Email address", defaultValue: "anna@example.com")]
    let file = mine.appending(path: "labelled.zip").path
    _ = try exportBlockPackage(project: mine.path, blockFile: Self.blockFile, share: share, destination: file, asZip: true)
    let model = BlockInstallModel(project: theirs)
    await model.read(file)
    await model.install()

    let entry = try #require(ProjectLibrary.communityEntries(theirs).first)
    #expect(entry.labels == ["email": "Email address"])
    #expect(entry.defaults == ["email": "anna@example.com"])
  }

  @Test func asksWhatToDoWithAnInstalledBlockOfAnotherVersion() async throws {
    let (mine, theirs) = try projects()
    let (old, new) = (try zip("1.0.0", from: mine), try zip("1.1.0", from: mine))
    let model = BlockInstallModel(project: theirs)
    await model.read(old)
    await model.install()

    await model.read(old)
    #expect(model.conflict == .same(version: "1.0.0"))
    await model.read(new)
    #expect(model.conflict == .update(from: "1.0.0", to: "1.1.0"))
    await model.install()
    #expect(listCommunityBlocks(project: theirs.path).map(\.manifest.version) == ["1.1.0"])

    await model.read(old)
    #expect(model.conflict == .downgrade(from: "1.1.0", to: "1.0.0"))
    model.startOver()
    #expect(listCommunityBlocks(project: theirs.path).map(\.manifest.version) == ["1.1.0"], "Nothing changed")
  }

  @Test func startingOverThrowsAwayWhatWasUnpacked() async throws {
    let (mine, theirs) = try projects()
    let model = BlockInstallModel(project: theirs)
    await model.read(try zip("1.0.0", from: mine))
    let preview = try #require(model.preview)
    #expect(preview.temporary)
    #expect(FileManager.default.fileExists(atPath: preview.folder))
    model.startOver()
    #expect(!FileManager.default.fileExists(atPath: preview.folder))
    if case .choosing = model.phase {} else { Issue.record("Expected to be choosing again") }
  }

  @Test func saysInWordsWhyABlockCantBeRead() async throws {
    let theirs = try folder()
    let model = BlockInstallModel(project: theirs)
    await model.read("http://example.com/block.zip")
    guard case .failed(let message) = model.phase else {
      Issue.record("Expected a failure, got \(model.phase)")
      return
    }
    #expect(message.contains("https"))
    #expect(message.contains("http://example.com/block.zip"))

    await model.read(theirs.appending(path: "nothing.zip").path)
    if case .failed = model.phase {} else { Issue.record("A file that isn't there can't be read") }
    await model.read("   ")
    if case .failed = model.phase {} else { Issue.record("An empty address changes nothing") }
  }

  @Test func readsOnlyWhatLooksLikeABlockWhenDropped() throws {
    let (mine, _) = try projects()
    let archive = try zip("1.0.0", from: mine)
    #expect(BlockInstallModel.source(of: URL(fileURLWithPath: archive)) == archive)
    #expect(BlockInstallModel.source(of: URL(string: "https://github.com/anna/sign-in")!) == "https://github.com/anna/sign-in")
    #expect(BlockInstallModel.source(of: URL(string: "http://github.com/anna/sign-in")!) == nil)
    #expect(BlockInstallModel.source(of: mine) == nil, "A project folder isn't a block")
    #expect(BlockInstallModel.source(of: mine.appending(path: Self.blockFile)) == nil)

    let package = try folder()
    try "{}".write(to: package.appending(path: "block.json"), atomically: true, encoding: .utf8)
    #expect(BlockInstallModel.source(of: package) == package.path)
  }

  @Test func asksBeforeRemovingABlockThatTestsUse() async throws {
    let (mine, theirs) = try projects()
    let model = BlockInstallModel(project: theirs)
    await model.read(try zip("1.0.0", from: mine))
    await model.install()
    let user = """
      {"format": 1, "name": "T", "app": "com.apple.TextEdit", "start": "a", "blocks": {
        "a": {"type": "runTest", "test": "blocks/com.anna.sign-in.jevblock/flow.json", "inputs": {"email": "a@b.c"}}}}
      """
    try user.write(to: theirs.appending(path: "t.flow.json"), atomically: true, encoding: .utf8)
    let entry = try #require(ProjectLibrary.communityEntries(theirs).first)

    let blocks = CustomBlocksModel(project: theirs)
    var changes = 0
    blocks.onChange = { changes += 1 }
    blocks.remove(entry)
    #expect(blocks.removal?.tests == ["t.flow.json"], "The tests that use it are named")
    #expect(listCommunityBlocks(project: theirs.path).count == 1, "It isn't removed behind anyone's back")
    #expect(CustomBlocksSheet.list(["a/Login.flow.json", "t.flow.json"]) == "Login, t")

    blocks.confirmRemoval()
    #expect(blocks.removal == nil)
    #expect(listCommunityBlocks(project: theirs.path).isEmpty)
    #expect(changes == 1)
  }

  @Test func removesAnUnusedBlockAtOnce() async throws {
    let (mine, theirs) = try projects()
    let model = BlockInstallModel(project: theirs)
    await model.read(try zip("1.0.0", from: mine))
    await model.install()
    let entry = try #require(ProjectLibrary.communityEntries(theirs).first)

    let blocks = CustomBlocksModel(project: theirs)
    blocks.remove(entry)
    #expect(blocks.removal == nil)
    #expect(blocks.error == nil)
    #expect(listCommunityBlocks(project: theirs.path).isEmpty)

    blocks.remove(entry)
    #expect(blocks.error?.contains("com.anna.sign-in") == true, "Removing it twice says it isn't installed")
  }

  /// A test in `theirs` with one step that runs the block `com.anna.sign-in`, as the file keeps it.
  private func userTest(in theirs: URL, package: String) throws -> FlowDocument {
    let text = """
      {"format": 1, "name": "T", "app": "com.apple.TextEdit", "start": "a", "blocks": {
        "a": {"type": "runTest", "test": "blocks/com.anna.sign-in.jevblock/flow.json"\(package),
              "inputs": {"email": "a@b.c"}}}}
      """
    let file = theirs.appending(path: "t.flow.json")
    try text.write(to: file, atomically: true, encoding: .utf8)
    return FlowDocument(url: file)
  }

  @Test func aMissingBlockIsAnErrorThatNamesItsVersionAndAddress() throws {
    let (_, theirs) = try projects()
    let address = "https://example.com/sign-in.zip"
    let document = try userTest(in: theirs, package: #", "package": {"version": "1.2.0", "source": "\#(address)"}"#)
    let issue = try #require(document.issues(of: "a").first { $0.code == "flow.block.notInstalled" })
    #expect(issue.severity == .error)
    #expect(IssueText.message(issue).contains("com.anna.sign-in 1.2.0"))
    #expect(BlockNeed(issue) == BlockNeed(id: "com.anna.sign-in", version: "1.2.0", source: address))

    let old = try userTest(in: theirs, package: "")
    let oldIssue = try #require(old.issues(of: "a").first { $0.code == "flow.block.notInstalled" })
    #expect(BlockNeed(oldIssue) == BlockNeed(id: "com.anna.sign-in"), "A test from before the version was kept still reads")
    #expect(BlockNeed(oldIssue)?.label == "com.anna.sign-in")
    #expect(IssueText.message(oldIssue).contains("“com.anna.sign-in”"))
  }

  @Test func theRunNamesTheBlockItNeedsToo() {
    let failure = Failure(
      code: "run.block.notInstalled",
      params: ["block": "a", "package": "com.anna.sign-in", "version": "1.2.0", "source": "https://example.com/b.zip"])
    #expect(BlockNeed(failure) == BlockNeed(id: "com.anna.sign-in", version: "1.2.0", source: "https://example.com/b.zip"))
    #expect(RunText.failure(failure).contains("com.anna.sign-in 1.2.0"))
    #expect(BlockNeed(Failure(code: "run.file.missing", params: [:])) == nil)
  }

  @Test func installingFromTheErrorChecksThatItIsTheBlockTheTestNeeds() async throws {
    let (mine, theirs) = try projects()
    let file = try zip("1.0.0", from: mine)
    let document = try userTest(in: theirs, package: #", "package": {"version": "1.0.0"}"#)
    let need = try #require(document.issues(of: "a").lazy.compactMap(BlockNeed.init).first)

    let model = BlockInstallModel(project: theirs)
    model.need = need
    await model.read(file)
    #expect(model.needMismatch == nil, "The same block and version")

    model.need = BlockNeed(id: "com.bob.other", version: "1.0.0")
    #expect(model.needMismatch == .otherBlock(wanted: "com.bob.other 1.0.0"))
    model.need = BlockNeed(id: "com.anna.sign-in", version: "2.0.0")
    #expect(model.needMismatch == .otherVersion(wanted: "2.0.0"))
    model.need = BlockNeed(id: "com.anna.sign-in")
    #expect(model.needMismatch == nil, "A test that names no version takes any")

    await model.install()
    document.revalidate()
    #expect(document.issues(of: "a").allSatisfy { $0.code != "flow.block.notInstalled" })
  }

  @Test func theErrorOpensTheSheetOnTheBlockAndAsksForAFileWithoutAnAddress() async throws {
    let (mine, theirs) = try projects()
    let file = try zip("1.0.0", from: mine)
    let project = ProjectStore()
    project.open(theirs)

    let withoutAddress = BlockNeed(id: "com.anna.sign-in", version: "1.0.0")
    project.installBlock(for: withoutAddress)
    var model = try #require(project.installingBlock)
    #expect(model.need == withoutAddress)
    if case .choosing = model.phase {} else { Issue.record("Without an address the sheet waits for a file, got \(model.phase)") }

    project.installingBlock = nil
    project.installBlock(for: BlockNeed(id: "com.anna.sign-in", version: "1.0.0", source: file))
    model = try #require(project.installingBlock)
    try await eventuallyAsync("the sheet reads the block") { isReview(model.phase) }
    #expect(isReview(model.phase), "With an address the sheet reads the block at once")
    #expect(model.needMismatch == nil)
    await model.install()
    #expect(ProjectLibrary.communityEntries(theirs).map { $0.package?.id } == ["com.anna.sign-in"])
  }

  @Test func aStepAddedFromTheLibraryKeepsTheVersionOfItsBlock() async throws {
    let (mine, theirs) = try projects()
    let model = BlockInstallModel(project: theirs)
    await model.read(try zip("1.0.0", from: mine))
    await model.install()
    let entry = try #require(ProjectLibrary.communityEntries(theirs).first)
    let document = try userTest(in: theirs, package: "")

    document.add(entry, at: Slot(from: "a", edge: "next"))
    let added = try #require(document.selection.flatMap(document.block))
    guard case .runTest(_, let package, _, _) = added.kind else {
      Issue.record("Expected a step that runs the block, got \(added.kind)")
      return
    }
    #expect(package == PackageNeed(version: "1.0.0", source: nil), "A block from a file has no address to keep")
    #expect(LibraryEntry.Package(id: "x", author: "", version: "1.0.0", license: "", source: "/tmp/x.zip").need.source == nil)
  }

  @Test func namesBlocksOfCommunityByTheirIdInSummaries() {
    let path = "blocks/com.anna.sign-in.jevblock/flow.json"
    #expect(ProjectScanner.testName(path: path) == "com.anna.sign-in")
    #expect(ProjectScanner.testName(path: "Login/Sign In.flow.json") == "Sign In")
    #expect(ProjectScanner.testName(path: "blocks/Sign in.block.json") == "Sign in")
    let summary = BlockSummary.text(.runTest(test: path, package: nil, inputs: [:], next: nil), appName: { $0 })
    #expect(summary.contains("com.anna.sign-in"))
  }

  @Test func sharingStartsFromTheBlockAndChecksTheId() {
    let entry = LibraryEntry(
      kind: .block, id: Self.blockFile, name: "Sign in", detail: "Types an email", symbol: "person", text: nil,
      url: nil, inputs: ["email"])
    var draft = ShareBlockDraft(entry: entry)
    #expect(!draft.canShare, "An author is needed")
    draft.author = "Anna Smith"
    #expect(draft.id == "com.anna-smith.sign-in")
    #expect(draft.canShare)
    #expect(draft.archiveName == "com.anna-smith.sign-in-1.0.0.zip")
    #expect(draft.share.inputs.map(\.name) == ["email"])
    #expect(draft.share.description == "Types an email")

    draft.customId = "sign-in"
    #expect(!draft.hasValidId)
    #expect(!draft.canShare)
  }

  @Test func offersToInstallBlocksOnlyInAProject() {
    var state = PaletteState()
    #expect(!CommandPalette.entries(state).map(\.action).contains(.installBlock))
    state.hasProject = true
    let found = CommandPalette.search("community", in: CommandPalette.entries(state)).map(\.action)
    #expect(found.contains(.installBlock))
    #expect(found.contains(.customBlocks))
  }

  /// `theirs` with the block of `mine` installed as `com.anna.sign-in`, its package in the blocks folder.
  private func installed() throws -> URL {
    let (mine, theirs) = try projects()
    _ = try exportBlockPackage(
      project: mine.path, blockFile: Self.blockFile, share: share("1.0.0"),
      destination: theirs.appending(path: "blocks").path, asZip: false)
    return theirs
  }

  /// The file the step `a` of `document` runs.
  private func runs(_ document: FlowDocument) -> String? {
    guard case .runTest(let test, _, _, _) = document.block("a")?.kind else { return nil }
    return test
  }

  @Test func anInstalledBlockOpensReadOnlyAndIsNeverWritten() throws {
    let theirs = try installed()
    let file = theirs.appending(path: "blocks/com.anna.sign-in.jevblock/flow.json")
    let before = try Data(contentsOf: file)
    let document = FlowDocument(url: file, project: theirs)
    let undo = UndoManager()
    document.undoManager = undo
    let flow = try #require(document.flow)
    #expect(document.isReadOnly)

    document.selection = "a"
    #expect(!document.startLink())
    document.insert("check", at: nil)
    document.addCondition()
    document.remove([.block(id: "a")])
    document.updateFlow(field: "app") { $0.app = "com.apple.Safari" }
    var draft = flow
    draft.name = "Draft"
    document.showDraft(draft)

    #expect(document.flow == flow)
    #expect(document.pendingRemoval == nil)
    #expect(!document.hasUnsavedChanges)
    #expect(!undo.canUndo)
    #expect(try Data(contentsOf: file) == before)
    #expect(try !userTest(in: theirs, package: "").isReadOnly)
  }

  @Test func makingABlockOwnCopiesItAndSwitchesTheTestsToTheCopy() throws {
    let theirs = try installed()
    let test = try userTest(in: theirs, package: "")
    let model = OwnBlockModel(project: theirs, id: "com.anna.sign-in", name: "Sign in")
    var opened: String?
    model.onMade = { opened = $0 }
    #expect(model.name == "Sign in")
    #expect(model.tests == ["t.flow.json"])
    #expect(model.switchesTests)

    model.make(open: [test])

    #expect(model.error == nil)
    #expect(model.made == MadeOwnBlock(file: "blocks/Sign in.block.json", skipped: []))
    #expect(opened == "blocks/Sign in.block.json")
    #expect(runs(test) == "blocks/Sign in.block.json", "The open test shows the copy at once")
    #expect(FileManager.default.fileExists(atPath: theirs.appending(path: "blocks/com.anna.sign-in.jevblock").path))
    #expect(!model.canMake, "Made once")
  }

  @Test func theCopyTakesTheNameGivenAndCanLeaveTheTestsAlone() throws {
    let theirs = try installed()
    try Self.block.write(to: theirs.appending(path: Self.blockFile), atomically: true, encoding: .utf8)
    let test = try userTest(in: theirs, package: "")
    let model = OwnBlockModel(project: theirs, id: "com.anna.sign-in", name: "Sign in")
    #expect(model.name == "Sign in 2", "The project has a Sign in block of its own")

    model.name = "  Log in "
    model.switchesTests = false
    model.make(open: [test])

    #expect(model.made == MadeOwnBlock(file: "blocks/Log in.block.json", skipped: []))
    let copy = try loadFlow(json: String(contentsOf: theirs.appending(path: "blocks/Log in.block.json"), encoding: .utf8))
    #expect(copy.name == "Log in")
    #expect(!FileManager.default.fileExists(atPath: theirs.appending(path: "blocks/Sign in 2.block.json").path))
    #expect(runs(test) == "blocks/com.anna.sign-in.jevblock/flow.json")
    #expect(ProjectLibrary.communityEntries(theirs).count == 1, "The installed block stays")
  }

  @Test func aTestWithChangesNotInItsFileKeepsTheInstalledBlock() throws {
    let theirs = try installed()
    let test = try userTest(in: theirs, package: "")
    let other = theirs.appending(path: "u.flow.json")
    try FileManager.default.copyItem(at: test.url, to: other)
    let before = try Data(contentsOf: test.url)
    var draft = try #require(test.flow)
    draft.name = "Draft"
    test.showDraft(draft)
    #expect(test.hasUnsavedChanges)

    let model = OwnBlockModel(project: theirs, id: "com.anna.sign-in", name: "Sign in")
    model.make(open: [test])

    #expect(model.made?.skipped == ["t.flow.json"])
    #expect(try Data(contentsOf: test.url) == before)
    #expect(test.flow?.name == "Draft", "The draft is still shown")
    #expect(runs(FlowDocument(url: other)) == "blocks/Sign in.block.json", "A test that isn't open switches")
  }

  @Test func aTestOpenOnTheBoardWithChangesKeepsTheInstalledBlockToo() throws {
    let theirs = try installed()
    _ = try userTest(in: theirs, package: "")
    let board = BoardTestDocuments()
    let store = ProjectStore()
    store.boardDocuments = board
    let shown = board.document(of: "t.flow.json", in: theirs)
    #expect(store.openDocuments.contains { $0 === shown }, "The board's tests count as open")
    let before = try Data(contentsOf: shown.url)
    var draft = try #require(shown.flow)
    draft.name = "Draft"
    shown.showDraft(draft)
    #expect(shown.hasUnsavedChanges)

    let model = OwnBlockModel(project: theirs, id: "com.anna.sign-in", name: "Sign in")
    model.make(open: store.openDocuments)

    #expect(model.made?.skipped == ["t.flow.json"], "The sheet names it")
    #expect(try Data(contentsOf: shown.url) == before)
    #expect(shown.flow?.name == "Draft", "The draft is still shown")
  }

  @Test func aBlockThatNeedsAMissingBlockOffersToInstallIt() throws {
    let (mine, theirs) = try projects()
    _ = try exportBlockPackage(
      project: mine.path, blockFile: Self.blockFile, share: share("1.0.0"),
      destination: mine.appending(path: "blocks").path, asZip: false)
    let wrap = """
      {"format": 1, "name": "Wrap", "app": "com.apple.TextEdit", "start": "a", "blocks": {
        "a": {"type": "runTest", "test": "com.anna.sign-in.jevblock/flow.json", "inputs": {"email": "a@b.c"}}}}
      """
    try wrap.write(to: mine.appending(path: "blocks/Wrap.block.json"), atomically: true, encoding: .utf8)
    var meta = share("1.0.0")
    meta.id = "com.anna.wrap"
    meta.inputs = []
    let exported = try exportBlockPackage(
      project: mine.path, blockFile: "blocks/Wrap.block.json", share: meta,
      destination: theirs.appending(path: "blocks").path, asZip: false)
    #expect(exported.manifest.dependencies.map(\.id) == ["com.anna.sign-in"])
    let test = """
      {"format": 1, "name": "T", "app": "com.apple.TextEdit", "start": "a", "blocks": {
        "a": {"type": "runTest", "test": "blocks/com.anna.wrap.jevblock/flow.json"}}}
      """
    let file = theirs.appending(path: "t.flow.json")
    try test.write(to: file, atomically: true, encoding: .utf8)

    let document = FlowDocument(url: file, project: theirs)
    let issue = try #require(document.issues(of: "a").first { $0.code == "flow.block.dependencyUnmet" })
    #expect(BlockNeed(issue) == BlockNeed(id: "com.anna.sign-in"))
  }

  @Test func sharingGivesTheInputsTheirLabelsAndDefaults() throws {
    let (mine, _) = try projects()
    let entry = LibraryEntry(
      kind: .block, id: Self.blockFile, name: "Sign in", detail: "", symbol: "person", text: nil, url: nil,
      inputs: ["email"], labels: ["email": "Email"])
    var draft = ShareBlockDraft(entry: entry)
    draft.author = "Anna"
    #expect(draft.labels["email"] == "Email")
    draft.labels["email"] = "  E-mail "
    draft.defaults["email"] = "me@example.com"
    let out = mine.appending(path: "out").path
    let shared = try exportBlockPackage(
      project: mine.path, blockFile: Self.blockFile, share: draft.share, destination: out, asZip: false)
    #expect(shared.manifest.inputs == [BlockPackageInput(name: "email", label: "E-mail", defaultValue: "me@example.com")])

    draft.labels["email"] = " "
    draft.defaults["email"] = ""
    #expect(draft.share.inputs == [BlockPackageInput(name: "email", label: nil, defaultValue: nil)])
    let plain = try exportBlockPackage(
      project: mine.path, blockFile: Self.blockFile, share: draft.share, destination: out, asZip: false)
    #expect(plain.manifest.inputs == [BlockPackageInput(name: "email", label: nil, defaultValue: nil)])

    draft.labels["email"] = String(repeating: "x", count: ShareBlockDraft.labelLimit + 1)
    #expect(draft.labelTooLong("email"))
    #expect(!draft.canShare, "The core takes 60 characters at most")
  }
}
