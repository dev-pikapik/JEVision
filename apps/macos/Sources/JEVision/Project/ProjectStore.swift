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

  private(set) var status: Status = .closed
  private(set) var rootURL: URL?
  private(set) var name = ""
  private(set) var items: [ProjectItem] = []
  /// The item selected in the sidebar.
  var selection: ProjectItem.ID? {
    didSet { if selection != oldValue { openSelection() } }
  }
  /// The selected test, open in the editor.
  private(set) var document: FlowDocument?
  /// The New Test sheet is shown.
  var isCreatingTest = false

  @ObservationIgnored private var watcher: FolderWatcher?
  @ObservationIgnored private var scanTask: Task<[ProjectItem], any Error>?
  @ObservationIgnored private var accessedURL: URL?

  private enum Key {
    static let bookmark = "projectBookmark"
    static let name = "projectName"
  }

  init() {
    guard UserDefaults.standard.data(forKey: Key.bookmark) != nil else { return }
    name = UserDefaults.standard.string(forKey: Key.name) ?? ""
    if let url = resolveBookmark() {
      open(url)
    } else {
      status = .unavailable
    }
  }

  func choose() {
    let panel = NSOpenPanel()
    panel.canChooseDirectories = true
    panel.canChooseFiles = false
    panel.allowsMultipleSelection = false
    panel.canCreateDirectories = true
    panel.message = String(localized: "Choose the folder that holds your tests.")
    panel.prompt = String(localized: "Open")
    guard panel.runModal() == .OK, let url = panel.url else { return }
    save(url)
    open(url)
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
    guard let rootURL, let selection, ProjectScanner.isTestFile(selection) else {
      document = nil
      return
    }
    let url = rootURL.appending(path: selection)
    if document?.url != url { document = FlowDocument(url: url) }
  }

  /// The folder new tests go to: the selected folder, or the folder of the selected test.
  private var targetFolder: (id: String, url: URL)? {
    guard let rootURL else { return nil }
    guard let selection, let item = items.item(withID: selection) else { return ("", rootURL) }
    let id = item.kind == .folder ? item.id : item.parentPath
    return (id, id.isEmpty ? rootURL : rootURL.appending(path: id))
  }

  /// Writes a new test with one Open App block and selects it.
  func createTest(name: String, app: String) throws {
    guard let (folderID, folder) = targetFolder else { return }
    let base = RelativePath.testFileName(name)
    let stem = ProjectScanner.testName(base)
    let fileName = RelativePath.unique(stem) {
      FileManager.default.fileExists(atPath: folder.appending(path: $0 + ProjectScanner.testSuffix).path)
    } + ProjectScanner.testSuffix
    let empty = Flow(
      name: name.trimmingCharacters(in: .whitespaces), description: nil, app: app, testType: nil, priority: nil,
      status: nil, tags: [], retries: 0, start: "", blocks: [])
    let flow = insertBlock(flow: empty, typeName: "openApp", slot: nil)?.flow ?? empty
    try Data(saveFlow(flow: flow).utf8).write(to: folder.appending(path: fileName), options: .withoutOverwriting)
    selection = folderID.isEmpty ? fileName : folderID + "/" + fileName
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
    try FileManager.default.moveItem(at: item.url, to: target)
    if let text = try? String(contentsOf: target, encoding: .utf8), var flow = try? loadFlow(json: text) {
      flow.name = name
      try? saveFlow(flow: flow).write(to: target, atomically: true, encoding: .utf8)
    }
    if selection == item.id {
      selection = item.parentPath.isEmpty ? target.lastPathComponent : item.parentPath + "/" + target.lastPathComponent
    }
  }

  func moveToTrash(_ item: ProjectItem) throws {
    try FileManager.default.trashItem(at: item.url, resultingItemURL: nil)
    if selection == item.id { selection = nil }
  }

  private func open(_ url: URL) {
    accessedURL?.stopAccessingSecurityScopedResource()
    accessedURL = url.startAccessingSecurityScopedResource() ? url : nil
    rootURL = url
    name = FileManager.default.displayName(atPath: url.path)
    selection = nil
    items = []
    status = .loading
    watcher = FolderWatcher(url: url) { [weak self] in self?.rescan() }
    rescan()
  }

  private func rescan() {
    guard let url = rootURL else { return }
    scanTask?.cancel()
    let task = Task.detached(priority: .userInitiated) { try ProjectScanner.scan(url) }
    scanTask = task
    Task { [weak self] in
      let result = await task.result
      guard let self, scanTask == task, !task.isCancelled else { return }
      switch result {
      case .success(let found):
        items = found
        status = .ready
        if let selection, found.item(withID: selection) == nil,
          !FileManager.default.fileExists(atPath: url.appending(path: selection).path)
        {
          self.selection = nil
        }
        document?.reload()
      case .failure:
        // A moved or renamed folder is still found through the bookmark.
        if let moved = resolveBookmark(), moved.standardizedFileURL != url.standardizedFileURL {
          open(moved)
        } else {
          items = []
          status = .unavailable
        }
      }
    }
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
