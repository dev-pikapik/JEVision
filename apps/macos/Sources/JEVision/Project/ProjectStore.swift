import AppKit
import JEVisionCore
import Observation

/// The project folder the user opened, remembered between launches and kept in sync with the disk.
@MainActor
@Observable
final class ProjectStore {
  enum Status: Equatable {
    case closed, loading, ready, unavailable
  }

  enum Sidebar: Hashable {
    case tests, runs
  }

  private(set) var status: Status = .closed
  private(set) var rootURL: URL?
  private(set) var name = ""
  private(set) var items: [ProjectItem] = []
  /// The item selected in the sidebar; selected again on the next launch.
  var selection: ProjectItem.ID? {
    didSet {
      guard selection != oldValue else { return }
      UserDefaults.standard.set(selection, forKey: Key.selection)
      if selection != nil { showsBoard = false }
      openSelection()
    }
  }
  /// The board in view fills the center instead of a test.
  var showsBoard = false
  /// The selected test, open in the editor.
  private(set) var document: FlowDocument?
  /// The New Test sheet is shown.
  var isCreatingTest = false
  /// The block the Install Block sheet is reading or installing; set to show the sheet.
  var installingBlock: BlockInstallModel?
  /// The Custom Blocks sheet is shown.
  var showsCustomBlocks = false
  /// The command palette (⌘K) is shown.
  var showsCommandPalette = false
  /// Saved runs, newest first.
  private(set) var runs: [SavedRun] = []
  /// The newest run of each test made on this Mac or started from it, by test file relative to
  /// the project: a CI run is of another checkout, and says nothing about the test here.
  private(set) var latestRuns: [String: SavedRun] = [:]
  /// Tests whose recent runs both passed and failed without the test changing, by test file
  /// relative to the project, with the evidence.
  private(set) var flaky: [String: FlakyEvidence] = [:]
  /// Tests that were unreliable at least once in the last week, by test file relative to the
  /// project; `quarantineSuggestion(for:)` keeps those worth putting in quarantine.
  private(set) var unreliable: [String: QuarantineSuggestion] = [:]
  /// Which tests the sidebar lists and Run All runs, by the fields of their cards.
  var filter = TestFilter()
  /// What the sidebar lists.
  var sidebar = Sidebar.tests
  /// The runs the Runs list shows by where they were started; all of them when `nil`.
  var runSource: RunSource?
  /// Folder of the run whose report is open.
  var selectedRun: SavedRun.ID?
  /// Counts reference images replaced or put back, so views that compare with them look again.
  private(set) var referenceChanges = 0
  /// The Accept New Reference wizard in view.
  var referenceWizard: ReferenceWizard?
  /// The test or folder, relative to the project, whose history is shown.
  var historyPath: String?
  /// Folder of the failed run whose bug report is being written.
  var bugReportRun: SavedRun.ID?
  /// The test, relative to the project, the Statistics window is about; `nil` for the whole
  /// project.
  var statisticsTest: String?
  let git = ProjectGit()
  let environments = ProjectEnvironments()
  let boards = ProjectBoards()
  let schedules = ProjectSchedules()
  let ci = ProjectCI()
  let library = ProjectLibrary()
  /// Settings shared through the project's settings file: default retries, the tests in
  /// quarantine and when to suggest it.
  private(set) var settings = ProjectSettings(retries: nil, quarantine: [])
  /// Why the project's settings file can't be read.
  private(set) var settingsProblem: String?

  @ObservationIgnored private var watcher: FolderWatcher?
  @ObservationIgnored private var scanTask: Task<([ProjectItem], History), any Error>?
  @ObservationIgnored private var accessedURL: URL?

  enum Key {
    static let bookmark = "projectBookmark"
    static let name = "projectName"
    static let selection = "projectSelection"
  }

  init() {
    ci.imported = { [weak self] in self?.rescan() }
    guard UserDefaults.standard.data(forKey: Key.bookmark) != nil else { return }
    name = UserDefaults.standard.string(forKey: Key.name) ?? ""
    let selected = UserDefaults.standard.string(forKey: Key.selection)
    if let url = resolveBookmark() {
      open(url)
      selection = selected.flatMap { FileManager.default.fileExists(atPath: url.appending(path: $0).path) ? $0 : nil }
    } else {
      status = .unavailable
      AppLog.error("The project folder from last time can’t be opened", "project")
    }
  }

  func choose() {
    let panel = NSOpenPanel()
    panel.canChooseDirectories = true
    panel.canChooseFiles = false
    panel.allowsMultipleSelection = false
    panel.canCreateDirectories = true
    panel.message = String(localized: "Choose the folder that holds your tests")
    panel.directoryURL = rootURL ?? URL.documentsDirectory
    panel.prompt = String(localized: "Open")
    guard panel.runModal() == .OK, let url = panel.url else { return }
    save(url)
    open(url)
  }

  /// Shows the Install Block sheet, reading the block at `source` (a folder, a .zip file or an
  /// address) at once when one is given.
  func installBlock(from source: String? = nil) {
    installingBlock = makeBlockInstaller(source: source)
  }

  /// Shows the Install Block sheet for a block a test needs: it reads the block from the address the
  /// test knows, or waits for a file or an address when it doesn't know one.
  func installBlock(for need: BlockNeed) {
    installingBlock = makeBlockInstaller(source: need.source)
    installingBlock?.need = need
  }

  /// A sheet model that installs blocks into this project: it reads `source` at once, or the
  /// update of the installed block `id`, when one is given.
  func makeBlockInstaller(source: String? = nil, updating id: String? = nil) -> BlockInstallModel? {
    guard let rootURL else { return nil }
    let model = BlockInstallModel(project: rootURL)
    model.onInstalled = { [weak self] _ in self?.blocksChanged() }
    if let id {
      Task { await model.readUpdate(of: id) }
    } else if let source {
      Task { await model.read(source) }
    }
    return model
  }

  func makeCustomBlocks() -> CustomBlocksModel? {
    guard let rootURL else { return nil }
    let model = CustomBlocksModel(project: rootURL)
    model.onChange = { [weak self] in self?.blocksChanged() }
    return model
  }

  /// The blocks of the project changed: the library shows them, and the open test is checked again.
  private func blocksChanged() {
    library.reload()
    document?.revalidate()
  }

  func reveal(_ item: ProjectItem) {
    NSWorkspace.shared.activateFileViewerSelecting([item.url])
  }

  func revealRoot() {
    guard let rootURL else { return }
    NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: rootURL.path)
  }

  // MARK: Tests

  private func openSelection() {
    guard let rootURL, let selection,
      ProjectScanner.isTestFile(selection) || selection.lowercased().hasSuffix(ProjectLibrary.blockSuffix)
    else {
      document = nil
      return
    }
    let url = rootURL.appending(path: selection)
    if document?.url != url { document = FlowDocument(url: url, project: rootURL) }
  }

  /// The folder new tests go to: the selected folder, or the folder of the selected test.
  private var targetFolder: (id: String, url: URL)? {
    guard let rootURL else { return nil }
    guard let selection, let item = items.item(withID: selection) else { return ("", rootURL) }
    let id = item.kind == .folder ? item.id : item.parentPath
    return (id, id.isEmpty ? rootURL : rootURL.appending(path: id))
  }

  /// Writes a new test with one Open App block, signed with the git name, and selects it.
  func createTest(name: String, app: String) async throws {
    let author = await git.identity().name.trimmingCharacters(in: .whitespaces)
    guard let (folderID, folder) = targetFolder else { return }
    let base = RelativePath.testFileName(name)
    let stem = ProjectScanner.testName(base)
    let fileName = RelativePath.unique(stem) {
      FileManager.default.fileExists(atPath: folder.appending(path: $0 + ProjectScanner.testSuffix).path)
    } + ProjectScanner.testSuffix
    let empty = Flow(
      name: name.trimmingCharacters(in: .whitespaces), description: nil, icon: nil, app: app, testType: nil, priority: nil,
      status: nil, tags: [], author: author.isEmpty ? nil : author, retries: nil, start: "", blocks: [],
      canvas: CanvasContent(positions: [:], sections: [], notes: []))
    let flow = insertBlock(flow: empty, typeName: "openApp", slot: nil)?.flow ?? empty
    try Data(saveFlow(flow: flow).utf8).write(to: folder.appending(path: fileName), options: .withoutOverwriting)
    selection = folderID.isEmpty ? fileName : folderID + "/" + fileName
  }

  /// Where a first project goes when the person hasn't chosen a folder.
  static var defaultFolder: URL { URL.documentsDirectory.appending(path: "JEVision Tests", directoryHint: .isDirectory) }

  /// Opens the default project folder, making it first if needed.
  func openDefaultFolder() throws {
    let url = Self.defaultFolder
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    save(url)
    open(url)
  }

  /// Puts the demo test in the open project, or in the default one, and selects it.
  func openDemo() throws {
    if rootURL == nil { try openDefaultFolder() }
    guard let rootURL else { return }
    selection = try DemoTest.write(into: rootURL)
  }

  /// Copies the example project into a folder the person chooses and opens the copy; nothing
  /// happens when they cancel.
  func openExample() throws {
    guard let source = ExampleProject.bundled() else { throw ExampleProject.Failure.missing }
    let panel = NSOpenPanel()
    panel.canChooseDirectories = true
    panel.canChooseFiles = false
    panel.allowsMultipleSelection = false
    panel.canCreateDirectories = true
    panel.message = String(localized: "Choose where to put a copy of the example project")
    panel.directoryURL = URL.documentsDirectory
    panel.prompt = String(localized: "Copy Here")
    guard panel.runModal() == .OK, let parent = panel.url else { return }
    let copy = try ExampleProject.copy(source, into: parent)
    AppLog.info("Example project copied to \(copy.path)", "project")
    save(copy)
    open(copy)
    selection = ExampleProject.firstTest
  }

  /// Renames the test file and the name inside it.
  func rename(_ item: ProjectItem, to newName: String) throws {
    let name = newName.trimmingCharacters(in: .whitespaces)
    let target = item.url.deletingLastPathComponent().appending(path: RelativePath.testFileName(name))
    guard !name.isEmpty, target.lastPathComponent != item.url.lastPathComponent else { return }
    let caseOnly = target.lastPathComponent.lowercased() == item.url.lastPathComponent.lowercased()
    if !caseOnly, FileManager.default.fileExists(atPath: target.path) {
      throw CocoaError(.fileWriteFileExists, userInfo: [NSFilePathErrorKey: target.path])
    }
    let assets = noteAssets(of: item)
    try FileManager.default.moveItem(at: item.url, to: target)
    if let assets {
      do {
        try FileManager.default.moveItem(at: assets, to: Self.noteAssetsURL(of: target))
      } catch {
        try? FileManager.default.moveItem(at: target, to: item.url)
        throw error
      }
    }
    if let text = try? String(contentsOf: target, encoding: .utf8), var flow = try? loadFlow(json: text) {
      flow.name = name
      try? saveFlow(flow: flow).write(to: target, atomically: true, encoding: .utf8)
    }
    let id = item.parentPath.isEmpty ? target.lastPathComponent : item.parentPath + "/" + target.lastPathComponent
    boards.renamed(item.id, to: id)
    if selection == item.id { selection = id }
  }

  /// The folder with the files that come with the notes of test `item`, when it has one: it goes
  /// with the test.
  private func noteAssets(of item: ProjectItem) -> URL? {
    guard item.kind == .test else { return nil }
    let folder = Self.noteAssetsURL(of: item.url)
    return FileManager.default.fileExists(atPath: folder.path) ? folder : nil
  }

  private static func noteAssetsURL(of test: URL) -> URL {
    URL(filePath: noteAssetsFolder(test: test.path(percentEncoded: false)), directoryHint: .isDirectory)
  }

  /// Shows the board `name` in the center.
  func showBoard(_ name: String) {
    boards.select(name)
    sidebar = .tests
    showsBoard = true
    selection = nil
  }

  func moveToTrash(_ item: ProjectItem) throws {
    let assets = noteAssets(of: item)
    try FileManager.default.trashItem(at: item.url, resultingItemURL: nil)
    if let assets { try? FileManager.default.trashItem(at: assets, resultingItemURL: nil) }
    if selection == item.id { selection = nil }
  }

  // MARK: Runs

  /// Opens the report of the run saved in `dir`.
  func showRun(_ dir: URL) {
    sidebar = .runs
    // `path`, not `path(percentEncoded:)`: that one keeps the slash after a folder, and then the
    // run no longer matches its row in the history, which is picked by the folder without it.
    selectedRun = dir.path
  }

  /// Whether the test file, relative to the project, is still there.
  func hasTest(_ file: String) -> Bool {
    guard let rootURL else { return false }
    return FileManager.default.fileExists(atPath: rootURL.appending(path: file).path)
  }

  /// Opens the test in the editor with the block selected; with `run`, the blocks show how they
  /// did in that run.
  func showBlock(_ block: String, in file: String, from run: SavedRun.ID? = nil) {
    sidebar = .tests
    selection = file
    document?.selection = block
    if let run { document?.reportRun = run }
  }

  func moveToTrash(_ run: SavedRun) throws {
    try FileManager.default.trashItem(at: run.url, resultingItemURL: nil)
    show(History(runs.filter { $0.id != run.id }))
    if selectedRun == run.id { selectedRun = nil }
  }

  /// Looks again at the reference images and at the tests open, after a reference was
  /// replaced or put back, with the settings of the blocks that use it.
  func referencesChanged() {
    referenceChanges += 1
    document?.reload()
  }

  // MARK: Settings

  /// The quarantine of a test file, relative to the project, while it lasts: an entry whose last
  /// day is past is kept in the settings file but puts nothing in quarantine.
  func quarantine(of file: String) -> Quarantined? {
    let today = localToday()
    return settings.quarantine.first { $0.test == file && quarantineActive(entry: $0, today: today) }
  }

  /// Why the test is worth putting in quarantine: it was unreliable as many times in the last
  /// week as the project's threshold says, and isn't in quarantine yet.
  func quarantineSuggestion(for file: String) -> QuarantineSuggestion? {
    guard let suggestion = unreliable[file], quarantine(of: file) == nil,
      suggestion.events.count >= Int(quarantineThreshold(settings: settings))
    else { return nil }
    return suggestion
  }

  /// The test file `url` as the quarantine names it: relative to the project, or `nil` outside it.
  func testID(_ url: URL) -> String? {
    guard let rootURL, RelativePath.isInside(url, folder: rootURL) else { return nil }
    return RelativePath.from(rootURL, to: url)
  }

  /// How many times the test is retried when it fails: its own number, else the project's.
  func retries(of flow: Flow) -> UInt32 {
    flow.retries ?? settings.retries ?? 0
  }

  /// Puts the test in quarantine until the end of the day `until` (`YYYY-MM-DD`), or without an
  /// end: it still runs, but its failures don't fail Run All, the command line or the JUnit
  /// report. A test already in quarantine gets the new reason and end.
  func quarantine(_ file: String, reason: String?, until: String?) throws {
    try changeSettings { try quarantineTest(project: $0, test: file, reason: reason, until: until) }
  }

  /// Puts the test in quarantine as its suggestion asks: for the usual two weeks, with what made
  /// it unreliable as the reason.
  func quarantine(_ file: String, accepting suggestion: QuarantineSuggestion) throws {
    try quarantine(
      file, reason: QuarantineText.suggestion(suggestion), until: defaultQuarantineEnd(today: localToday()))
  }

  func releaseFromQuarantine(_ file: String) throws {
    try changeSettings { try JEVisionCore.releaseFromQuarantine(project: $0, test: file) }
  }

  /// After how many unreliable runs within a week quarantine is suggested; `nil` for the usual 3.
  func setQuarantineThreshold(_ after: UInt32?) throws {
    try changeSettings { try JEVisionCore.setQuarantineThreshold(project: $0, after: after) }
  }

  /// How many times a failed test is retried when its file doesn't say; `nil` retries none.
  func setDefaultRetries(_ retries: UInt32?) throws {
    try changeSettings { try JEVisionCore.setDefaultRetries(project: $0, retries: retries) }
  }

  /// How the tests of `folder`, relative to the project (`.` for Run All), run together.
  func runMode(of folder: String) -> RunMode {
    folderRunMode(settings: settings, folder: folder)
  }

  /// Sets how the tests of `folder` run together; saved in the project settings file, so everyone
  /// who runs the project gets the same.
  func setRunMode(_ mode: RunMode, of folder: String) throws {
    try changeSettings { try setFolderRunMode(project: $0, folder: folder, mode: mode) }
  }

  /// The AI the project asks for `task`; `nil` leaves it to Settings.
  func setAi(_ task: AiTask, _ choice: AiChoice?) throws {
    try changeSettings { try setProjectAi(project: $0, task: task, choice: choice) }
  }

  /// Whether screenshots of the project's apps may go to cloud models.
  func setLocalOnly(_ localOnly: Bool) throws {
    try changeSettings { try setProjectLocalOnly(project: $0, localOnly: localOnly) }
  }

  /// The areas of the window, in fractions, covered in every screenshot of the project. Undo
  /// puts the previous areas back; with `undoManager` nil the change joins the last undo step.
  func setHiddenAreas(_ hide: [Rect], undoManager: UndoManager?) throws {
    let old = settings.hide
    guard hide != old else { return }
    try changeSettings { try setProjectHide(project: $0, hide: hide) }
    undoManager?.registerUndo(withTarget: self) { store in
      MainActor.assumeIsolated { try? store.setHiddenAreas(old, undoManager: undoManager) }
    }
    undoManager?.setActionName(String(localized: "Change Hidden Areas"))
  }

  private func changeSettings(_ change: (String) throws -> ProjectSettings) throws {
    guard let rootURL else { return }
    settings = try change(rootURL.path(percentEncoded: false))
    settingsProblem = nil
  }

  private func reloadSettings() {
    guard let rootURL else { return }
    do {
      settings = try projectSettings(project: rootURL.path(percentEncoded: false))
      settingsProblem = nil
    } catch {
      settings = ProjectSettings(retries: nil, quarantine: [])
      settingsProblem = Self.message(error)
    }
  }

  /// Why the project's settings couldn't be read or changed, for an alert.
  static func message(_ error: any Error) -> String {
    switch error {
    case ProjectSettingsError.Unreadable(let file, let reason):
      String(localized: "\(file) can’t be read: \(reason)")
    case ProjectSettingsError.Unwritable(let file, let reason):
      String(localized: "\(file) can’t be saved: \(reason)")
    case ProjectSettingsError.Rejected(let reason):
      reason
    case ReferenceError.Unreadable(let test, let reason):
      String(localized: "\(test) can’t be read: \(reason)")
    case ReferenceError.NoSuchPicture(let test, _, _):
      String(localized: "\(test) has changed: a block that used the picture is gone.")
    case ReferenceError.Rejected(let test, _, let reason):
      String(localized: "The settings for \(test) don’t fit: \(reason)")
    case ReferenceError.Changed(let test):
      String(localized: "\(test) was changed meanwhile, so nothing was replaced.")
    case ReferenceError.Unwritable(let test, let reason):
      String(localized: "\(test) can’t be saved: \(reason)")
    default:
      error.localizedDescription
    }
  }

  // MARK: History

  /// Writes `path` (relative to the project) as `source` was in `commit`. The current file goes
  /// to the Trash; Undo brings it back.
  func restore(_ path: String, from source: String, in commit: String, undoManager: UndoManager?) throws {
    guard let rootURL, let file = Baseline.file(path, in: rootURL) else {
      throw CocoaError(.fileWriteNoPermission, userInfo: [NSFilePathErrorKey: path])
    }
    let files = FileManager.default
    let old = files.fileExists(atPath: file.path) ? try Baseline.moveToTrash(file) : nil
    do {
      try gitRestore(project: rootURL.path(percentEncoded: false), commit: commit, source: source, path: path)
    } catch {
      if let old { try? files.moveItem(at: old, to: file) }
      throw error
    }
    let action = String(localized: "Restore Version")
    undoManager?.registerUndo(withTarget: self) { store in
      MainActor.assumeIsolated {
        guard (try? Baseline.restore(old, to: file)) != nil else { return }
        undoManager?.registerUndo(withTarget: store) { store in
          MainActor.assumeIsolated {
            try? store.restore(path, from: source, in: commit, undoManager: undoManager)
          }
        }
        undoManager?.setActionName(action)
      }
    }
    undoManager?.setActionName(action)
  }

  func open(_ url: URL) {
    accessedURL?.stopAccessingSecurityScopedResource()
    accessedURL = url.startAccessingSecurityScopedResource() ? url : nil
    AppLog.info("Project opened: \(url.path)", "project")
    rootURL = url
    name = FileManager.default.displayName(atPath: url.path)
    selection = nil
    showsBoard = false
    showsCommandPalette = false
    items = []
    filter = TestFilter()
    show(History([]))
    selectedRun = nil
    runSource = nil
    sidebar = .tests
    status = .loading
    watcher = FolderWatcher(url: url) { [weak self] in self?.rescan() }
    git.open(url)
    environments.open(url)
    boards.open(url)
    schedules.open(url)
    ci.open(url)
    library.open(url)
    rescan()
  }

  private func rescan() {
    guard let url = rootURL else { return }
    environments.reload()
    boards.reload()
    schedules.reload()
    library.reload()
    reloadSettings()
    document?.revalidate()
    scanTask?.cancel()
    let task = Task.detached(priority: .userInitiated) {
      let items = try ProjectScanner.scan(url)
      // Runs are a bonus to the tests: an unreadable history doesn't close the project.
      return (items, History((try? listRuns(project: url.path)) ?? []))
    }
    scanTask = task
    Task { [weak self] in
      let result = await task.result
      guard let self, scanTask == task, !task.isCancelled else { return }
      switch result {
      case .success(let (found, history)):
        // The folder changes all the time while a test runs; unchanged lists don't redraw the sidebar.
        if items != found { items = found }
        boards.sync(tests: ProjectMap.groups(found, rootTitle: name).flatMap(\.tests).map(\.id), rootTitle: name)
        show(history)
        let saved = history.runs
        if let selectedRun, !saved.contains(where: { $0.id == selectedRun }),
          !FileManager.default.fileExists(atPath: selectedRun)
        {
          self.selectedRun = nil
        }
        status = .ready
        if let selection, found.item(withID: selection) == nil,
          !FileManager.default.fileExists(atPath: url.appending(path: selection).path)
        {
          self.selection = nil
        }
        document?.reload()
        git.refresh()
      case .failure:
        // A moved or renamed folder is still found through the bookmark.
        if let moved = resolveBookmark(), moved.standardizedFileURL != url.standardizedFileURL {
          open(moved)
        } else {
          items = []
          show(History([]))
          status = .unavailable
        }
      }
    }
  }

  /// Saved runs and what the project shows from them, worked out off the main thread: in a
  /// project with a long history that takes long enough to stall the window.
  private struct History: Sendable {
    let runs: [SavedRun]
    let latest: [String: SavedRun]
    let flaky: [String: FlakyEvidence]
    let unreliable: [String: QuarantineSuggestion]

    init(_ runs: [SavedRun]) {
      self.runs = runs
      latest = ProjectMap.latestRuns(runs.filter { $0.report.ci == nil })
      let reports = runs.map(\.report)
      let now = Int64(Date.now.timeIntervalSince1970 * 1000)
      flaky = runs.isEmpty ? [:] : flakyTests(runs: reports, nowMs: now)
      // Every test unreliable even once: the threshold is applied when it is asked for, so
      // changing it in Settings needs no new look at the runs.
      unreliable = runs.isEmpty ? [:] : quarantineSuggestions(runs: reports, nowMs: now, after: 1)
    }
  }

  private func show(_ history: History) {
    if runs != history.runs { runs = history.runs }
    if latestRuns != history.latest { latestRuns = history.latest }
    if flaky != history.flaky { flaky = history.flaky }
    if unreliable != history.unreliable { unreliable = history.unreliable }
  }

  private func save(_ url: URL) {
    let bookmark = try? url.bookmarkData(options: .withSecurityScope, includingResourceValuesForKeys: nil, relativeTo: nil)
    UserDefaults.standard.set(bookmark, forKey: Key.bookmark)
    UserDefaults.standard.set(FileManager.default.displayName(atPath: url.path), forKey: Key.name)
  }

  private func resolveBookmark() -> URL? {
    guard let data = UserDefaults.standard.data(forKey: Key.bookmark) else { return nil }
    var stale = false
    // The security scope is tied to the app's signature, so a re-signed build can't use it.
    // The app isn't sandboxed, so the plain bookmark still finds the folder; it is then saved again.
    let scoped = try? URL(resolvingBookmarkData: data, options: .withSecurityScope, bookmarkDataIsStale: &stale)
    guard let url = scoped ?? (try? URL(resolvingBookmarkData: data, options: [], bookmarkDataIsStale: &stale)),
      FileManager.default.fileExists(atPath: url.path)
    else { return nil }
    if stale || scoped == nil { save(url) }
    return url
  }
}
