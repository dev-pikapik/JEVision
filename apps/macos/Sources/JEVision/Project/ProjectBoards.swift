import AppKit
import ImageIO
import JEVisionCore
import JEVisionHands
import Observation

/// The boards of the open project: `*.canvas` files in its boards folder, one of them in view.
/// One board holds every test by itself; the rest hold what people put on them. The board in
/// view is kept on this Mac, per project.
@MainActor
@Observable
final class ProjectBoards {
  /// The boards' names, as their files are called without the ending, in name order.
  private(set) var names: [String] = []
  /// The board in view.
  private(set) var current: String?
  /// What the board in view holds; `nil` while its file can't be read.
  private(set) var board: Board?
  /// What shows of `board` and where, worked out once per change.
  private(set) var scene = BoardScene.empty
  /// Counts the changes of `board`: what is worked out from the scene starts over when it moves.
  private(set) var generation = 0
  /// Why the file of the board in view can't be read.
  private(set) var problem: String?
  /// The board that holds every test of the project.
  private(set) var allTests: String?
  /// The board whose name is being changed, for the Rename Board alert.
  var renaming: String?
  /// Something the board in view should do, asked from the command palette.
  var request: BoardRequest?
  /// Where each board was last looked at, so going back to it shows the same place.
  @ObservationIgnored var viewports: [String: BoardViewport] = [:]
  @ObservationIgnored private var project: URL?
  @ObservationIgnored private let defaults: UserDefaults
  /// Each board file as last read or written, with whether it is the board of all tests: a board
  /// is read again only when its file changes.
  @ObservationIgnored private var known: [String: (stamp: FileStamp?, allTests: Bool)] = [:]

  static let selectionKey = "selectedBoards"
  nonisolated static let suffix = boardSuffix()
  nonisolated static let folderName = boardsFolder()

  init(defaults: UserDefaults = Profile.current.defaults) {
    self.defaults = defaults
  }

  var folder: URL? { project?.appending(path: Self.folderName, directoryHint: .isDirectory) }

  /// What shows of `board`.
  var layout: BoardLayout { scene.layout }

  /// The file of the board `name`, relative to the project, as boards point at each other.
  nonisolated static func path(of name: String) -> String { folderName + "/" + name + suffix }

  /// The board name of a file relative to the project, when it is a board of the project.
  nonisolated static func name(of path: String) -> String? {
    let prefix = folderName + "/"
    guard path.hasPrefix(prefix), path.hasSuffix(suffix) else { return nil }
    let name = String(path.dropFirst(prefix.count).dropLast(suffix.count))
    return name.isEmpty || name.contains("/") ? nil : name
  }

  func open(_ url: URL) {
    project = url
    known = [:]
    viewports = [:]
    renaming = nil
    request = nil
    reload()
  }

  /// Reads the boards folder and the boards whose files changed, after something changed on
  /// disk. Returns whether anything did since the app last read or wrote them.
  @discardableResult
  func reload() -> Bool {
    guard let folder else { return false }
    let files = (try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? []
    let found = files.filter { $0.hasSuffix(Self.suffix) && $0.count > Self.suffix.count }
      .map { String($0.dropLast(Self.suffix.count)) }
      .sorted { $0.localizedStandardCompare($1) == .orderedAscending }
    var changed = names != found
    if changed { names = found }
    var read: [String: Board] = [:]
    var stale: Set<String> = []
    var known: [String: (stamp: FileStamp?, allTests: Bool)] = [:]
    for name in found {
      let stamp = FileStamp(path: file(name).path)
      if let old = self.known[name], old.stamp == stamp {
        known[name] = old
        continue
      }
      stale.insert(name)
      read[name] = try? self.read(name)
      known[name] = (stamp, read[name]?.allTests == true)
    }
    changed = changed || !stale.isEmpty
    self.known = known
    let flagged = found.first { known[$0]?.allTests == true }
    if allTests != flagged { allTests = flagged }
    let saved = (defaults.dictionary(forKey: Self.selectionKey) as? [String: String])?[project?.path ?? ""]
    let shown = saved.flatMap { found.contains($0) ? $0 : nil } ?? flagged ?? found.first
    // The board in view is read again only when its file changed or another board is shown.
    if shown != current || board == nil || shown.map(stale.contains) == true { show(shown, read: shown.flatMap { read[$0] }) }
    return changed
  }

  /// Puts the tests `tests` (relative to the project, in sidebar order) on the board of all
  /// tests and takes the deleted ones off it. A project with tests and no such board gets one.
  func sync(tests: [String], rootTitle: String) {
    guard let folder else { return }
    if allTests == nil {
      guard !tests.isEmpty else { return }
      let name = RelativePath.unique(String(localized: "All Tests"), exists: names.contains)
      let empty = Board(nodes: [], edges: [], allTests: true, extra: "")
      do {
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try write(empty, to: name)
      } catch {
        return
      }
      reload()
      guard allTests == name else { return }
    }
    guard let name = allTests, let board = name == current ? board : try? read(name),
      let synced = syncBoardTests(board: board, tests: tests, rootTitle: rootTitle)
    else { return }
    try? write(synced, to: name)
    if name == current { show(synced) }
  }

  func select(_ name: String) {
    guard let project, names.contains(name) else { return }
    var saved = defaults.dictionary(forKey: Self.selectionKey) as? [String: String] ?? [:]
    saved[project.path] = name
    defaults.set(saved, forKey: Self.selectionKey)
    show(name)
  }

  /// Makes an empty board, named after `name` as a file can be, and returns its name.
  @discardableResult
  func create(_ name: String) throws -> String {
    guard let folder else { throw CocoaError(.fileNoSuchFile) }
    let name = RelativePath.unique(Self.fileName(name), exists: names.contains)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    try write(Board(nodes: [], edges: [], allTests: false, extra: ""), to: name)
    reload()
    select(name)
    return name
  }

  /// Gives the board `old` the name `new`; boards that point at it follow.
  func rename(_ old: String, to new: String) throws {
    guard let project, let folder else { return }
    let name = Self.fileName(new)
    guard name != old, names.contains(old) else { return }
    let caseOnly = name.lowercased() == old.lowercased()
    if !caseOnly, names.contains(where: { $0.lowercased() == name.lowercased() }) {
      throw CocoaError(.fileWriteFileExists, userInfo: [NSFilePathErrorKey: folder.appending(path: name + Self.suffix).path])
    }
    try FileManager.default.moveItem(at: file(old), to: file(name))
    _ = renameOnBoards(project: project.path(percentEncoded: false), old: Self.path(of: old), new: Self.path(of: name))
    viewports[name] = viewports.removeValue(forKey: old)
    let wasCurrent = current == old
    reload()
    if wasCurrent { select(name) }
  }

  /// Moves the board `name` to the Trash. The board of all tests stays: it is the project's.
  func moveToTrash(_ name: String) throws {
    guard name != allTests else { return }
    try FileManager.default.trashItem(at: file(name), resultingItemURL: nil)
    viewports[name] = nil
    if current == name, let project {
      var saved = defaults.dictionary(forKey: Self.selectionKey) as? [String: String] ?? [:]
      saved[project.path] = nil
      defaults.set(saved, forKey: Self.selectionKey)
    }
    reload()
  }

  /// Points every board at `new` where it showed the file or folder `old`, after a rename.
  func renamed(_ old: String, to new: String) {
    guard let project else { return }
    if !renameOnBoards(project: project.path(percentEncoded: false), old: old, new: new).isEmpty { reload() }
  }

  /// Replaces the board in view with `new` and saves it; Undo puts the old one back.
  func edit(_ new: Board, action: String, undoManager: UndoManager?) throws {
    guard let name = current, let old = board, new != old else { return }
    try put(new, in: name, replacing: old, action: action, undoManager: undoManager)
  }

  private func put(_ new: Board, in name: String, replacing old: Board, action: String, undoManager: UndoManager?) throws {
    try write(new, to: name)
    if name == current { show(new) }
    undoManager?.registerUndo(withTarget: self) { [weak undoManager] boards in
      MainActor.assumeIsolated {
        try? boards.put(old, in: name, replacing: new, action: action, undoManager: undoManager)
      }
    }
    undoManager?.setActionName(action)
  }

  /// Keeps the picture `data` in the project, puts it on the board in view centered on `point` and
  /// returns the new node's id. `name` is what the picture is called, with its ending.
  @discardableResult
  func addImage(_ data: Data, named name: String, at point: CGPoint, undoManager: UndoManager?) throws -> String? {
    guard let project, let board else { return nil }
    guard let size = Self.pixelSize(of: data) else {
      throw BoardError.NotAPicture
    }
    let file = try storeBoardImage(project: project.path(percentEncoded: false), name: name, data: data)
    let edit = addImageToBoard(board: board, file: file, width: size.width, height: size.height, at: Point(x: point.x, y: point.y))
    try self.edit(edit.board, action: String(localized: "Add Image"), undoManager: undoManager)
    return edit.id
  }

  /// The size of a picture in pixels, `nil` when the data isn't one.
  static func pixelSize(of data: Data) -> CGSize? {
    guard let source = CGImageSourceCreateWithData(data as CFData, nil),
      let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
      let width = properties[kCGImagePropertyPixelWidth] as? Double,
      let height = properties[kCGImagePropertyPixelHeight] as? Double, width > 0, height > 0
    else { return nil }
    return CGSize(width: width, height: height)
  }

  /// Opens the card of the test `id` so it shows its blocks, or folds them back into the card.
  func toggleBlocks(of id: String, undoManager: UndoManager?) throws {
    guard let project, let board, let node = scene.nodes[id] else { return }
    let new =
      node.open
      ? closeTestOnBoard(board: board, id: id)
      : openTestOnBoard(project: project.path(percentEncoded: false), board: board, id: id)
    guard let new else { return }
    try edit(
      new, action: node.open ? String(localized: "Hide Blocks") : String(localized: "Show Blocks"),
      undoManager: undoManager)
  }

  /// A file name for a board called `name`: characters a file name can't hold become dashes.
  static func fileName(_ name: String) -> String {
    let clean = name.trimmingCharacters(in: .whitespaces)
      .replacing(/[\/:\\]/, with: "-")
      .trimmingCharacters(in: CharacterSet(charactersIn: "."))
    return clean.isEmpty ? String(localized: "Untitled Board") : clean
  }

  /// Shows the board `name`, as `read` when it was just read.
  private func show(_ name: String?, read: Board? = nil) {
    if current != name { current = name }
    guard let name else {
      show(nil as Board?)
      problem = nil
      return
    }
    do {
      show(try read ?? self.read(name))
      if problem != nil { problem = nil }
    } catch {
      show(nil as Board?)
      problem = Self.message(error)
    }
  }

  private func show(_ new: Board?) {
    guard board != new else { return }
    board = new
    scene = new.map { BoardScene($0) } ?? .empty
    generation += 1
  }

  private func file(_ name: String) -> URL {
    (folder ?? URL(filePath: "/")).appending(path: name + Self.suffix)
  }

  private func read(_ name: String) throws -> Board {
    try loadBoard(text: String(contentsOf: file(name), encoding: .utf8))
  }

  private func write(_ board: Board, to name: String) throws {
    let file = file(name)
    try saveBoard(board: board).write(to: file, atomically: true, encoding: .utf8)
    // What the app wrote itself isn't read back when the folder watcher hears of it.
    known[name] = (FileStamp(path: file.path), board.allTests)
  }

  static func message(_ error: any Error) -> String {
    if case BoardError.Invalid(let message) = error {
      return String(localized: "This board can’t be read: \(message)")
    }
    if case BoardError.NotAPicture = error { return String(localized: "This isn’t a picture JEVision can show.") }
    return error.localizedDescription
  }
}

/// Where the board is looked at from: a board point `p` shows at `p × scale + offset`.
struct BoardViewport: Equatable {
  var offset = CGPoint.zero
  var scale = 1.0
}

/// Something the board in view does when the command palette asks.
enum BoardRequest: Hashable {
  case add(BoardAddition)
  /// Shows the node with this id in the middle and selects it.
  case reveal(String)
  case fit
}

/// What can be put on a board in one step.
enum BoardAddition: Hashable {
  case test, note, frame, link, board, image
}
