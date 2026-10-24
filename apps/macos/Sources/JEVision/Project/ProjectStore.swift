import AppKit
import JEVisionCore
import JEVisionHands
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
  /// Every test of `items`, at any depth, in sidebar order.
  private(set) var tests: [ProjectItem] = []
  private var itemsByID: [ProjectItem.ID: ProjectItem] = [:]
  /// The item selected in the sidebar; selected again on the next launch.
  var selection: ProjectItem.ID? {
    didSet {
      guard selection != oldValue else { return }
      defaults.set(selection, forKey: Key.selection)
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
  /// What the New Project sheet is asked to start with; set to show it.
  var newProject: NewProjectRequest?
  /// Projects opened on this Mac, pinned first, then the most recently opened.
  private(set) var recent: [RecentProject] = []
  /// What the cards on the Projects screen show, by project folder.
  private(set) var summaries: [String: ProjectSummary] = [:]
  /// The block the Install Block sheet is reading or installing; set to show the sheet.
  var installingBlock: BlockInstallModel?
  /// The installed block being copied among the project's own blocks; set to show the sheet.
  var owningBlock: OwnBlockModel?
  /// The blocks jevision.blocks.json names that aren't installed on this Mac.
  private(set) var unrestored: [UnrestoredBlock] = []
  /// The person said Not Now to installing them; the offer comes back when the project opens again.
  var restoreDeclined = false
  /// The Custom Blocks sheet is shown.
  var showsCustomBlocks = false
  /// The command palette (⌘K) is shown.
  var showsCommandPalette = false
  /// Saved runs, newest first.
  private(set) var runs: [SavedRun] = []
  /// `runs` by day, newest first.
  private(set) var runDays: [RunHistory.Day] = []
  /// Finds the runs a test shows among `runs`.
  private(set) var runIndex = RunIndex()
  /// The runs are being read for the first time since the project opened.
  private(set) var isLoadingRuns = false
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
  private(set) var settings = ProjectSettings(retries: nil, quarantine: []) {
    didSet { quarantined = nil }
  }
  /// Why the project's settings file can't be read.
  private(set) var settingsProblem: String?

  /// The tests open on the board while it is shown; the board owns them.
  @ObservationIgnored weak var boardDocuments: BoardTestDocuments?
  @ObservationIgnored private var watcher: FolderWatcher?
  @ObservationIgnored private var scanTask: Task<Scan?, any Error>?
  /// What the scans of the open project read from its tests.
  @ObservationIgnored private(set) var scanCache = ProjectScanner.Cache()
  /// The cache's stamp after the scan `items` came from.
  @ObservationIgnored private var shownScan = -1
  /// The tests last put on the board of all tests; `nil` when it must be looked at again.
  @ObservationIgnored private var syncedTests: [String]?
  @ObservationIgnored private(set) var runsFolder: RunsFolder?
  @ObservationIgnored private var runsRead: Task<Void, Never>?
  @ObservationIgnored private var isReadingRuns = false
  /// Runs changed while they were being read: `true` asks to read them again at once.
  @ObservationIgnored private var runsChangedMeanwhile: Bool?
  /// Counts the changes of `runDays`.
  @ObservationIgnored private var runsStamp = 0
  @ObservationIgnored private var shownDays: (stamp: Int, query: String, source: RunSource?, days: [RunHistory.Day])?
  /// The quarantine of each test, and the threshold, as the settings say until `until`.
  @ObservationIgnored private var quarantined: (until: Date, byTest: [String: Quarantined], threshold: Int)?
  @ObservationIgnored private(set) var accessedURL: URL?
  @ObservationIgnored private nonisolated(unsafe) var runSavedObserver: (any NSObjectProtocol)?
  @ObservationIgnored private(set) var summaryTask: Task<Void, Never>?
  @ObservationIgnored private let defaults: UserDefaults
  /// When a project is opened, for its place in the recent list.
  @ObservationIgnored private let clock: () -> Date
  /// The test running or being recorded now, which leaving the project would cut short; the
  /// app tells.
  @ObservationIgnored var activeWork: @MainActor () -> ProjectWork? = { nil }
  /// Asks whether to stop `activeWork` before leaving the project; `true` to stop it.
  @ObservationIgnored var asksToStop: @MainActor (ProjectWork) -> Bool = ProjectWork.ask

  enum Key {
    /// The recent projects, as the core writes the list.
    static let recent = "recentProjects"
    /// The folder of the project open when the app quit, opened again at launch.
    static let current = "openProject"
    static let selection = "projectSelection"
    /// Where versions before the recent list kept their one project; moved into the list once.
    static let bookmark = "projectBookmark"
    static let name = "projectName"
    /// The folder of “My Tests”, once made: it is known by where it is, as its name is in the
    /// language the app had then.
    static let myTests = "myTestsProject"
  }

  deinit {
    if let runSavedObserver { NotificationCenter.default.removeObserver(runSavedObserver) }
  }

  init(defaults: UserDefaults = Profile.current.defaults, clock: @escaping () -> Date = { .now }) {
    self.defaults = defaults
    self.clock = clock
    ci.imported = { [weak self] in self?.refreshRuns(soon: true) }
    runSavedObserver = NotificationCenter.default.addObserver(forName: RunCenter.runSaved, object: nil, queue: .main) {
      [weak self] note in
      let project = note.object as? URL
      MainActor.assumeIsolated {
        guard let self, let project, project.standardizedFileURL.path == self.rootURL?.standardizedFileURL.path else { return }
        self.refreshRuns(soon: true)
      }
    }
    migrate()
    recent = recentProjectsDecode(json: defaults.string(forKey: Key.recent) ?? "")
    refreshSummaries()
    guard let path = defaults.string(forKey: Key.current) else { return }
    let entry = recent.first { $0.path == path }
    guard let url = entry.flatMap(Self.folder) ?? Self.existing(URL(filePath: path, directoryHint: .isDirectory)) else {
      // The Projects screen shows its card as missing, with a way to find it.
      AppLog.error("The project folder from last time can’t be found", "project")
      return
    }
    let selected = defaults.string(forKey: Key.selection)
    if let entry, url.path != entry.path { relocate(entry.path, to: url) }
    // Opened like any other time, so a moved folder is opened there next time too.
    openProject(url)
    selection = selected.flatMap { FileManager.default.fileExists(atPath: url.appending(path: $0).path) ? $0 : nil }
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
    openProject(url)
  }

  /// Opens the project in `url`, puts it first among the recent projects and opens it again at
  /// the next launch. `test`, relative to the project, is selected when given.
  /// Opening another project asks first while a test runs or is being recorded.
  func openProject(_ url: URL, test: String? = nil) {
    guard let rootURL, rootURL.path != url.path else { return enter(url, test: test) }
    leave { [weak self] in self?.enter(url, test: test) }
  }

  private func enter(_ url: URL, test: String?) {
    remember(url)
    if rootURL?.path != url.path || status == .unavailable { open(url) }
    if let test {
      sidebar = .tests
      selection = test
    }
  }

  var sidebarSelection: ProjectSidebarSelection? {
    guard let path = rootURL?.path else { return nil }
    if sidebar == .runs { return selectedRun.map(ProjectSidebarSelection.run) ?? .project(path) }
    if showsBoard, let name = boards.current { return .board(name) }
    return selection.map { .item(project: path, file: $0) } ?? .project(path)
  }

  func selectSidebar(_ destination: ProjectSidebarSelection?) {
    switch destination {
    case .project(let path): openRecent(path)
    case .item(let path, let file):
      if rootURL?.path == path { selection = file }
      else { openRecent(path, test: file) }
    case .board(let name): showBoard(name)
    case .run(let id): selectedRun = id
    case nil:
      if sidebar == .runs { selectedRun = nil } else { selection = nil }
    }
  }

  /// Opens what the Finder, the Dock or a drop on the window hands over: a folder opens the
  /// project around it, a jevision.project.json its folder, a test its project at that test.
  /// `false` for anything else.
  @discardableResult
  func openItem(_ url: URL) -> Bool {
    guard let location = projectLocationFor(path: url.path) else { return false }
    openProject(URL(filePath: location.root, directoryHint: .isDirectory), test: location.test)
    return true
  }

  /// Opens the project of the recent list in `path`, where its folder is now. When the folder
  /// can't be found, says so and offers to find it.
  func openRecent(_ path: String, test: String? = nil) {
    guard let entry = recent.first(where: { $0.path == path }) else { return }
    if let url = Self.folder(of: entry) {
      if url.path != entry.path { relocate(entry.path, to: url) }
      openProject(url, test: test)
      return
    }
    summaries[path] = Self.missing(entry)
    let alert = NSAlert()
    alert.messageText = String(localized: "Can’t open “\(entry.name)”")
    alert.informativeText = String(localized: "The folder was moved, deleted or is on a disk that isn’t connected.")
    alert.addButton(withTitle: String(localized: "Locate…"))
    alert.addButton(withTitle: String(localized: "Cancel"))
    if alert.runModal() == .alertFirstButtonReturn, let url = locate(path) { openProject(url, test: test) }
  }

  /// Closes the project and shows the Projects screen, which the next launch shows too. Asks
  /// first while a test runs or is being recorded.
  func close() {
    leave { [weak self] in
      guard let self else { return }
      defaults.removeObject(forKey: Key.current)
      shut()
      refreshSummaries()
    }
  }

  /// Does `change`, closing the project or opening another, at once when no test runs or is
  /// being recorded; else once `readyToLeave` says so.
  private func leave(then change: @escaping () -> Void) {
    guard rootURL != nil, activeWork() != nil else { return change() }
    Task { if await readyToLeave() { change() } }
  }

  /// Whether the project can be left: at once when none is open or no test runs or is being
  /// recorded; else the person is asked, and once they agree it is stopped and waited for.
  /// `false` when they would rather it went on.
  func readyToLeave() async -> Bool {
    guard rootURL != nil, let work = activeWork() else { return true }
    guard asksToStop(work) else { return false }
    work.stop()
    while activeWork() != nil { try? await Task.sleep(for: .milliseconds(50)) }
    return true
  }

  /// Leaves the project: stops watching and using its folder and forgets what was shown of it.
  private func shut() {
    accessedURL?.stopAccessingSecurityScopedResource()
    accessedURL = nil
    watcher = nil
    scanTask?.cancel()
    scanTask = nil
    rootURL = nil
    name = ""
    selection = nil
    document = nil
    showsBoard = false
    items = []
    filter = TestFilter()
    show(Scan(items: [], stamp: -1, boardTests: []))
    show(Runs(RunsFolder.Found(runs: [], index: RunIndex(), changed: [])))
    selectedRun = nil
    runSource = nil
    sidebar = .tests
    historyPath = nil
    bugReportRun = nil
    statisticsTest = nil
    referenceWizard = nil
    unrestored = []
    restoreDeclined = false
    settings = ProjectSettings(retries: nil, quarantine: [])
    settingsProblem = nil
    git.open(nil)
    ci.open(nil)
    status = .closed
  }

  // MARK: Recent projects

  /// The recent projects whose folders were there when last looked at, for the menus.
  var openableRecent: [RecentProject] { Array(recent.filter { summaries[$0.path]?.exists != false }.prefix(30)) }

  /// How menus name `project`: by its name, with its folder when another project has the same name.
  func menuTitle(_ project: RecentProject) -> String {
    guard recent.contains(where: { $0.name == project.name && $0.path != project.path }) else { return project.name }
    return "\(project.name) — \(Self.shortPath(URL(filePath: project.path).deletingLastPathComponent().path))"
  }

  /// `path` as people read it, with `~` for the home folder.
  static func shortPath(_ path: String) -> String {
    (path as NSString).abbreviatingWithTildeInPath
  }

  func setPinned(_ path: String, _ pinned: Bool) {
    save(recentProjectsSetPinned(list: recent, path: path, pinned: pinned))
  }

  /// Takes the project off the list; its folder stays where it is.
  func removeRecent(_ path: String) {
    save(recentProjectsRemoved(list: recent, path: path))
    summaries[path] = nil
  }

  /// Clear Menu: keeps only the pinned projects.
  func clearRecent() {
    save(recentProjectsCleared(list: recent))
  }

  /// Asks where the folder of the recent project in `path` is now and moves its place in the
  /// list there; `nil` when the person cancels.
  @discardableResult
  func locate(_ path: String) -> URL? {
    guard let entry = recent.first(where: { $0.path == path }) else { return nil }
    let panel = NSOpenPanel()
    panel.canChooseDirectories = true
    panel.canChooseFiles = false
    panel.allowsMultipleSelection = false
    panel.message = String(localized: "Choose where the folder of “\(entry.name)” is now")
    panel.directoryURL = Self.existingParent(of: URL(filePath: path))
    panel.prompt = String(localized: "Choose")
    guard panel.runModal() == .OK, let url = panel.url else { return nil }
    relocate(path, to: url)
    return url
  }

  /// The project in `path` is in `url` now: it keeps its pin and its place in the list.
  func relocate(_ path: String, to url: URL) {
    move(path, to: url)
    _ = lookAt(recent.filter { $0.path == url.path })
  }

  private func move(_ path: String, to url: URL) {
    if defaults.string(forKey: Key.myTests) == path { defaults.set(url.path, forKey: Key.myTests) }
    save(recentProjectsRelocated(
      list: recent, oldPath: path, newPath: url.path, name: FileManager.default.displayName(atPath: url.path),
      bookmark: Self.bookmark(url)))
    summaries[path] = nil
  }

  /// Looks at the recent projects again for their cards and the menus, and follows folders that
  /// moved on the same disk.
  func refreshSummaries() {
    summaryTask?.cancel()
    summaryTask = lookAt(recent)
  }

  private func lookAt(_ list: [RecentProject]) -> Task<Void, Never> {
    Task { [weak self] in
      for entry in list {
        let (summary, moved) = await Task.detached(priority: .utility) { () -> (ProjectSummary, URL?) in
          let summary = projectSummary(root: entry.path)
          guard !summary.exists, let moved = Self.folder(of: entry), moved.path != entry.path else { return (summary, nil) }
          return (projectSummary(root: moved.path), moved)
        }.value
        guard let self, !Task.isCancelled else { return }
        if let moved {
          move(entry.path, to: moved)
          summaries[moved.path] = summary
        } else if recent.contains(where: { $0.path == entry.path }) {
          summaries[entry.path] = summary
        }
      }
    }
  }

  private func remember(_ url: URL) {
    save(recentProjectsOpened(
      list: recent, path: url.path, name: FileManager.default.displayName(atPath: url.path), bookmark: Self.bookmark(url),
      now: Int64(clock().timeIntervalSince1970)))
    defaults.set(url.path, forKey: Key.current)
  }

  private func save(_ list: [RecentProject]) {
    recent = list
    defaults.set(recentProjectsEncode(list: list), forKey: Key.recent)
  }

  /// Moves the one project of versions before the recent list into it, to open again at launch.
  private func migrate() {
    guard defaults.object(forKey: Key.recent) == nil, let bookmark = defaults.data(forKey: Key.bookmark) else { return }
    // The path is read from the bookmark even when the folder is gone, so its card can say so.
    if let path = URL.resourceValues(forKeys: [.pathKey], fromBookmarkData: bookmark)?.path {
      save(recentProjectsOpened(
        list: [], path: path, name: defaults.string(forKey: Key.name) ?? "", bookmark: bookmark,
        now: Int64(clock().timeIntervalSince1970)))
      defaults.set(path, forKey: Key.current)
    }
    defaults.removeObject(forKey: Key.bookmark)
    defaults.removeObject(forKey: Key.name)
  }

  private static func bookmark(_ url: URL) -> Data {
    (try? url.bookmarkData(options: .withSecurityScope, includingResourceValuesForKeys: nil, relativeTo: nil))
      ?? (try? url.bookmarkData()) ?? Data()
  }

  /// Where the folder of `entry` is now: where its bookmark leads, which follows a folder moved
  /// or renamed on the same disk, else its path. `nil` when it's gone, or in the Trash.
  nonisolated static func folder(of entry: RecentProject) -> URL? {
    var stale = false
    // The security scope is tied to the app's signature, so a re-signed build can't use it.
    // The app isn't sandboxed, so the plain bookmark still finds the folder; opening it saves a new one.
    let options: URL.BookmarkResolutionOptions = [.withoutUI, .withoutMounting]
    let found =
      (try? URL(resolvingBookmarkData: entry.bookmark, options: options.union(.withSecurityScope), bookmarkDataIsStale: &stale))
      ?? (try? URL(resolvingBookmarkData: entry.bookmark, options: options, bookmarkDataIsStale: &stale))
    if let found = found.flatMap(existing), !found.pathComponents.contains(where: { $0 == ".Trash" || $0 == ".Trashes" }) {
      return found
    }
    return existing(URL(filePath: entry.path, directoryHint: .isDirectory))
  }

  private nonisolated static func existing(_ url: URL) -> URL? {
    var isFolder: ObjCBool = false
    return FileManager.default.fileExists(atPath: url.path, isDirectory: &isFolder) && isFolder.boolValue ? url : nil
  }

  /// The nearest folder above `url` that is there, to start looking for it from.
  private static func existingParent(of url: URL) -> URL {
    var folder = url.deletingLastPathComponent()
    while folder.path != "/", existing(folder) == nil { folder = folder.deletingLastPathComponent() }
    return folder
  }

  private static func missing(_ entry: RecentProject) -> ProjectSummary {
    ProjectSummary(exists: false, name: entry.name, testCount: 0, branch: nil, lastRun: nil, folders: [])
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

  /// Shows the Install Block sheet with the blocks jevision.blocks.json names and this Mac hasn't got.
  func restoreBlocks() {
    installingBlock = makeBlockInstaller(restoring: true)
  }

  /// A sheet model that installs blocks into this project: it reads `source` at once, the update of
  /// the installed block `id`, or the blocks to restore, when one is given.
  func makeBlockInstaller(
    source: String? = nil, updating id: String? = nil, restoring: Bool = false
  ) -> BlockInstallModel? {
    guard let rootURL else { return nil }
    let model = BlockInstallModel(project: rootURL)
    model.onInstalled = { [weak self] _ in self?.blocksChanged() }
    if restoring {
      Task { await model.restore() }
    } else if let id {
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

  /// The tests shown for editing, in the editor and on the board: a change of a test's file must not
  /// overwrite what they show.
  var openDocuments: [FlowDocument] { [document].compactMap { $0 } + (boardDocuments?.open ?? []) }

  /// A sheet model that copies the installed block `id` among the project's own blocks and opens the copy.
  func makeOwnBlockModel(of id: String) -> OwnBlockModel? {
    guard let rootURL else { return nil }
    let name = library.entries.first { $0.package?.id == id }?.name ?? id
    let model = OwnBlockModel(project: rootURL, id: id, name: name)
    model.onMade = { [weak self] file in
      self?.blocksChanged()
      self?.selection = file
    }
    return model
  }

  /// The blocks of the project changed: the library shows them, and the open test is checked again.
  private func blocksChanged() {
    library.reload()
    reloadUnrestored()
    document?.revalidate()
  }

  private func reloadUnrestored() {
    unrestored = rootURL.flatMap { try? unrestoredBlocks(project: $0.path) } ?? []
  }

  func reveal(_ item: ProjectItem) {
    NSWorkspace.shared.activateFileViewerSelecting([item.url])
  }

  func revealRoot() {
    guard let rootURL else { return }
    NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: rootURL.path)
  }

  // MARK: Tests

  /// The folder or test with this id.
  func item(withID id: ProjectItem.ID) -> ProjectItem? { itemsByID[id] }

  private func openSelection() {
    guard let rootURL, let selection,
      ProjectScanner.isTestFile(selection) || selection.lowercased().hasSuffix(ProjectLibrary.blockSuffix)
        || blockPackageId(path: selection) != nil
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
    guard let selection, let item = item(withID: selection) else { return ("", rootURL) }
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

  /// Opens the project tests go to when the person hasn't opened one: “My Tests” in Documents ›
  /// JEVision of `home`, a project like any new one, made the first time and named `name`.
  func openMyTests(home: URL = .homeDirectory, name: String = String(localized: "My Tests")) throws {
    if let path = defaults.string(forKey: Key.myTests) {
      let entry = recent.first { $0.path == path }
      if let made = entry.flatMap(Self.folder) ?? Self.existing(URL(filePath: path, directoryHint: .isDirectory)) {
        if let entry, made.path != entry.path { relocate(entry.path, to: made) }
        return openProject(made)
      }
    }
    let parent = projectLocations(home: home.path(percentEncoded: false))[0]
    var folder = URL(filePath: parent, directoryHint: .isDirectory).appending(path: name, directoryHint: .isDirectory)
    if let problem = projectNameProblem(parent: parent, name: name) {
      // Made before, or a folder of that name with something in it: that is the one.
      guard Self.existing(folder) != nil else { throw ProjectFolderError.Failed(reason: problem) }
    } else {
      folder = URL(filePath: try createProject(parent: parent, name: name, starter: .empty).path, directoryHint: .isDirectory)
      AppLog.info("Project made: \(folder.path)", "project")
    }
    defaults.set(folder.path, forKey: Key.myTests)
    openProject(folder)
  }

  /// The name of “My Tests” for the person: its folder's once made, else the one it will get.
  var myTestsName: String {
    let made = defaults.string(forKey: Key.myTests).flatMap { Self.existing(URL(filePath: $0, directoryHint: .isDirectory)) }
    return made?.lastPathComponent ?? String(localized: "My Tests")
  }

  /// Puts the demo test in the open project, or in “My Tests”, and selects it.
  func openDemo() throws {
    if rootURL == nil { try openMyTests() }
    guard let rootURL else { return }
    selection = try DemoTest.write(into: rootURL)
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
    // The list lets go of it at once; what the runs say about the tests follows from the next read.
    runs.removeAll { $0.id == run.id }
    runDays = runDays.compactMap { day in
      var day = day
      day.runs.removeAll { $0.id == run.id }
      return day.runs.isEmpty ? nil : day
    }
    runsStamp += 1
    if selectedRun == run.id { selectedRun = nil }
    refreshRuns(soon: true)
  }

  /// The runs the Runs list shows: of tests whose name contains `query`, started from `source`
  /// (any when `nil`), newest first, by day.
  func runDays(matching query: String, from source: RunSource?) -> [RunHistory.Day] {
    let all = runDays
    if let shown = shownDays, shown.stamp == runsStamp, shown.query == query, shown.source == source { return shown.days }
    let days = RunHistory.days(all, matching: query, from: source)
    shownDays = (runsStamp, query, source, days)
    return days
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
    quarantine.byTest[file]
  }

  /// Why the test is worth putting in quarantine: it was unreliable as many times in the last
  /// week as the project's threshold says, and isn't in quarantine yet.
  func quarantineSuggestion(for file: String) -> QuarantineSuggestion? {
    let quarantine = quarantine
    guard let suggestion = unreliable[file], quarantine.byTest[file] == nil, suggestion.events.count >= quarantine.threshold
    else { return nil }
    return suggestion
  }

  /// What the settings put in quarantine today, worked out once a day and settings rather than
  /// once per row.
  private var quarantine: (byTest: [String: Quarantined], threshold: Int) {
    let settings = settings
    if let quarantined, Date.now < quarantined.until { return (quarantined.byTest, quarantined.threshold) }
    let today = localToday()
    var byTest: [String: Quarantined] = [:]
    for entry in settings.quarantine where byTest[entry.test] == nil && quarantineActive(entry: entry, today: today) {
      byTest[entry.test] = entry
    }
    let threshold = Int(quarantineThreshold(settings: settings))
    let tomorrow = Calendar.current.date(byAdding: .day, value: 1, to: Calendar.current.startOfDay(for: .now)) ?? .distantFuture
    quarantined = (tomorrow, byTest, threshold)
    return (byTest, threshold)
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

  /// Why the project, its folder or its settings couldn't be made, read or changed, for an alert.
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
    case ProjectFolderError.Failed(let reason):
      reason
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
    show(Scan(items: [], stamp: -1, boardTests: []))
    filter = TestFilter()
    show(Runs(RunsFolder.Found(runs: [], index: RunIndex(), changed: [])))
    selectedRun = nil
    runSource = nil
    sidebar = .tests
    status = .loading
    restoreDeclined = false
    watcher = FolderWatcher(url: url) { [weak self] folders in self?.changed(Change(folders)) }
    git.open(url)
    environments.open(url)
    boards.open(url)
    schedules.open(url)
    ci.open(url)
    library.open(url)
    reloadUnrestored()
    reloadSettings()
    scanCache = ProjectScanner.Cache()
    syncedTests = nil
    scanTests()
    runsRead?.cancel()
    runsRead = nil
    isReadingRuns = false
    runsChangedMeanwhile = nil
    runsFolder = RunsFolder(project: url)
    isLoadingRuns = true
    refreshRuns(soon: true)
  }

  /// What a change on the disk asks to read again.
  struct Change: OptionSet {
    let rawValue: Int
    static let tests = Change(rawValue: 1 << 0)
    /// The settings, environments and lock files at the top of the project.
    static let settings = Change(rawValue: 1 << 1)
    static let library = Change(rawValue: 1 << 2)
    static let boards = Change(rawValue: 1 << 3)
    static let schedules = Change(rawValue: 1 << 4)
    static let runs = Change(rawValue: 1 << 5)
    static let git = Change(rawValue: 1 << 6)
    static let everything: Change = [.tests, .settings, .library, .boards, .schedules, .runs, .git]

    init(rawValue: Int) { self.rawValue = rawValue }

    /// What changes in `folders`, relative to the project (`""` for the project folder itself), ask
    /// for; all of it when `folders` is `nil`. A run writes its report and pictures in the runs
    /// folder many times a second: that reads the runs again, not the tests.
    init(_ folders: Set<String>?) {
      guard let folders else {
        self = .everything
        return
      }
      self = []
      for folder in folders {
        let parts = folder.split(separator: "/")
        switch parts.first {
        case nil:
          formUnion(.everything)
        case ".jevision":
          if parts.count == 1 {
            formUnion([.runs, .schedules])
          } else if parts[1] == "runs" {
            formUnion(.runs)
          } else {
            formUnion(.schedules)
          }
        case ".git":
          formUnion(.git)
        // Tests are looked for outside hidden folders only.
        case _ where parts.contains(where: { $0.hasPrefix(".") }):
          continue
        case ProjectBoards.folderName?:
          formUnion([.boards, .tests, .git])
        case ProjectLibrary.blocksFolder?, ProjectLibrary.templatesFolder?:
          formUnion([.library, .tests, .git])
        default:
          formUnion([.tests, .git])
        }
      }
    }
  }

  /// Reads again what changed on the disk.
  private func changed(_ change: Change) {
    guard rootURL != nil else { return }
    if change.contains(.settings) {
      environments.reload()
      reloadSettings()
    }
    if change.contains(.boards), boards.reload() { syncedTests = nil }
    if change.contains(.schedules) { schedules.reload() }
    if change.contains(.library) { library.reload() }
    if !change.isDisjoint(with: [.settings, .library]) { reloadUnrestored() }
    if !change.isDisjoint(with: [.tests, .library]) { document?.revalidate() }
    if change.contains(.tests) { scanTests() }
    if change.contains(.runs) { refreshRuns() }
    if change.contains(.git) { git.refresh() }
  }

  /// The tests a scan found, with what the window shows of them worked out off the main thread.
  private struct Scan: Sendable {
    let items: [ProjectItem]
    let stamp: Int
    let tests: [ProjectItem]
    let byID: [ProjectItem.ID: ProjectItem]
    /// The tests, relative to the project, in the order the board of all tests shows them.
    let boardTests: [String]

    init(items: [ProjectItem], stamp: Int, boardTests: [String]) {
      self.items = items
      self.stamp = stamp
      tests = items.flatMap(\.tests)
      var byID: [ProjectItem.ID: ProjectItem] = [:]
      func add(_ items: [ProjectItem]) {
        for item in items {
          byID[item.id] = item
          add(item.children)
        }
      }
      add(items)
      self.byID = byID
      self.boardTests = boardTests
    }
  }

  private func scanTests() {
    guard let url = rootURL else { return }
    scanTask?.cancel()
    let (cache, shown, title) = (scanCache, shownScan, name)
    let task = Task.detached(priority: .userInitiated) { () throws -> Scan? in
      let (items, stamp) = try ProjectScanner.scan(url, cache: cache)
      // The folder changes all the time while a test runs; unchanged tests don't redraw anything.
      guard stamp != shown else { return nil }
      let boardTests = ProjectMap.groups(items, rootTitle: title).flatMap(\.tests).map(\.id)
      return Scan(items: items, stamp: stamp, boardTests: boardTests)
    }
    scanTask = task
    Task { [weak self] in
      let result = await task.result
      guard let self, scanTask == task, !task.isCancelled else { return }
      switch result {
      case .success(let scan):
        if let scan { show(scan) }
        if syncedTests != boardTests {
          boards.sync(tests: boardTests, rootTitle: name)
          syncedTests = boardTests
        }
        if status != .ready { status = .ready }
        if let selection, item(withID: selection) == nil,
          !FileManager.default.fileExists(atPath: url.appending(path: selection).path)
        {
          self.selection = nil
        }
        document?.reload()
      case .failure:
        // A moved or renamed folder is still found through the bookmark.
        if let entry = recent.first(where: { $0.path == url.path }), let moved = Self.folder(of: entry),
          moved.path != url.path
        {
          relocate(url.path, to: moved)
          enter(moved, test: nil)
        } else if Self.existing(url) != nil {
          // The folder is there but can't be read, such as when macOS hasn't let the app into it.
          show(Scan(items: [], stamp: -1, boardTests: []))
          show(Runs(RunsFolder.Found(runs: [], index: RunIndex(), changed: [])))
          status = .unavailable
        } else {
          // The Projects screen shows its card as missing, with a way to find it.
          AppLog.error("The project folder is gone", "project")
          shut()
          refreshSummaries()
        }
      }
    }
  }

  /// The tests the board of all tests shows, as the last scan found them.
  @ObservationIgnored private var boardTests: [String] = []

  private func show(_ scan: Scan) {
    shownScan = scan.stamp
    items = scan.items
    tests = scan.tests
    itemsByID = scan.byID
    boardTests = scan.boardTests
  }

  /// Reads the runs again, gathering the changes of a second into one read; `soon` reads them at
  /// once, as after a run of this app.
  func refreshRuns(soon: Bool = false) {
    guard let folder = runsFolder else { return }
    if isReadingRuns {
      runsChangedMeanwhile = soon || runsChangedMeanwhile == true
      return
    }
    if runsRead != nil {
      guard soon else { return }
      runsRead?.cancel()
    }
    runsRead = Task { [weak self] in
      if !soon { try? await Task.sleep(for: .seconds(1)) }
      guard !Task.isCancelled, let self else { return }
      isReadingRuns = true
      let runs = await Task.detached(priority: .userInitiated) { folder.read().map(Runs.init) }.value
      guard folder === runsFolder else { return }
      isReadingRuns = false
      runsRead = nil
      if let runs { show(runs) }
      isLoadingRuns = false
      if let soon = runsChangedMeanwhile {
        runsChangedMeanwhile = nil
        refreshRuns(soon: soon)
      }
    }
  }

  /// Saved runs and what the project shows from them, worked out off the main thread: in a
  /// project with a long history that takes long enough to stall the window.
  private struct Runs: Sendable {
    let found: RunsFolder.Found
    let days: [RunHistory.Day]
    let latest: [String: SavedRun]
    let flaky: [String: FlakyEvidence]
    let unreliable: [String: QuarantineSuggestion]

    init(_ found: RunsFolder.Found) {
      self.found = found
      let runs = found.runs
      days = RunHistory.days(runs)
      latest = ProjectMap.latestRuns(runs.filter(\.isHere))
      let reports = runs.map(\.report)
      let now = Int64(Date.now.timeIntervalSince1970 * 1000)
      flaky = runs.isEmpty ? [:] : flakyTests(runs: reports, nowMs: now)
      // Every test unreliable even once: the threshold is applied when it is asked for, so
      // changing it in Settings needs no new look at the runs.
      unreliable = runs.isEmpty ? [:] : quarantineSuggestions(runs: reports, nowMs: now, after: 1)
    }
  }

  private func show(_ new: Runs) {
    runs = new.found.runs
    runDays = new.days
    runsStamp += 1
    runIndex = new.found.index
    // Each test's row shows its last run: rows redraw only when one of those is another run or changed.
    let changed = new.found.changed
    if new.latest.count != latestRuns.count
      || new.latest.contains(where: { latestRuns[$0.key]?.id != $0.value.id || changed.contains($0.value.id) })
    {
      latestRuns = new.latest
    }
    if flaky != new.flaky { flaky = new.flaky }
    if unreliable != new.unreliable { unreliable = new.unreliable }
    if let selectedRun, runIndex.run(selectedRun) == nil, !FileManager.default.fileExists(atPath: selectedRun) {
      self.selectedRun = nil
    }
  }
}

/// What the New Project sheet starts with.
enum NewProjectRequest: Identifiable {
  /// An empty project in a new folder.
  case new
  /// A copy of a git repository.
  case clone
  /// A copy of the example project that comes with the app.
  case example

  var id: Self { self }
}
