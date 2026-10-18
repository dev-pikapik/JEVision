import AppKit
import JEVisionCore
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
  /// What shows of `board`, worked out once per change.
  private(set) var layout = BoardLayout(nodes: [], edges: [], bounds: nil)
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

  static let selectionKey = "selectedBoards"
  static let suffix = boardSuffix()
  static let folderName = boardsFolder()

  init(defaults: UserDefaults = .standard) {
    self.defaults = defaults
  }

  var folder: URL? { project?.appending(path: Self.folderName, directoryHint: .isDirectory) }

  /// The file of the board `name`, relative to the project, as boards point at each other.
  static func path(of name: String) -> String { folderName + "/" + name + suffix }

  /// The board name of a file relative to the project, when it is a board of the project.
  static func name(of path: String) -> String? {
    let prefix = folderName + "/"
    guard path.hasPrefix(prefix), path.hasSuffix(suffix) else { return nil }
    let name = String(path.dropFirst(prefix.count).dropLast(suffix.count))
    return name.isEmpty || name.contains("/") ? nil : name
  }

  func open(_ url: URL) {
    project = url
    viewports = [:]
    renaming = nil
    request = nil
    reload()
  }

  /// Reads the boards folder and the board in view again, after something changed on disk.
  func reload() {
    guard let folder else { return }
    let files = (try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? []
    let found = files.filter { $0.hasSuffix(Self.suffix) && $0.count > Self.suffix.count }
      .map { String($0.dropLast(Self.suffix.count)) }
      .sorted { $0.localizedStandardCompare($1) == .orderedAscending }
    if names != found { names = found }
    let flagged = found.first { (try? read($0))?.allTests == true }
    if allTests != flagged { allTests = flagged }
    let saved = (defaults.dictionary(forKey: Self.selectionKey) as? [String: String])?[project?.path ?? ""]
    show(saved.flatMap { found.contains($0) ? $0 : nil } ?? flagged ?? found.first)
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
    undoManager?.registerUndo(withTarget: self) { boards in
      MainActor.assumeIsolated {
        try? boards.put(old, in: name, replacing: new, action: action, undoManager: undoManager)
      }
    }
    undoManager?.setActionName(action)
  }

  /// A file name for a board called `name`: characters a file name can't hold become dashes.
  static func fileName(_ name: String) -> String {
    let clean = name.trimmingCharacters(in: .whitespaces)
      .replacing(/[\/:\\]/, with: "-")
      .trimmingCharacters(in: CharacterSet(charactersIn: "."))
    return clean.isEmpty ? String(localized: "Untitled Board") : clean
  }

  private func show(_ name: String?) {
    if current != name { current = name }
    guard let name else {
      show(nil as Board?)
      problem = nil
      return
    }
    do {
      show(try read(name))
      if problem != nil { problem = nil }
    } catch {
      show(nil as Board?)
      problem = Self.message(error)
    }
  }

  private func show(_ new: Board?) {
    guard board != new else { return }
    board = new
    layout = new.map { layOutBoard(board: $0) } ?? BoardLayout(nodes: [], edges: [], bounds: nil)
  }

  private func file(_ name: String) -> URL {
    (folder ?? URL(filePath: "/")).appending(path: name + Self.suffix)
  }

  private func read(_ name: String) throws -> Board {
    try loadBoard(text: String(contentsOf: file(name), encoding: .utf8))
  }

  private func write(_ board: Board, to name: String) throws {
    try saveBoard(board: board).write(to: file(name), atomically: true, encoding: .utf8)
  }

  static func message(_ error: any Error) -> String {
    if case BoardError.Invalid(let message) = error {
      return String(localized: "This board can’t be read: \(message)")
    }
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
  case test, note, frame, link, board
}
