import Foundation
import JEVisionCore
import Synchronization

/// Finds test files in a project folder. What is inside a test is read by the core: here, only
/// the words a search finds it by and its flaky mark.
enum ProjectScanner {
  static let testSuffix = ".flow.json"

  static func isTestFile(_ fileName: String) -> Bool {
    fileName.count > testSuffix.count && fileName.lowercased().hasSuffix(testSuffix)
  }

  /// The name of a test or custom block file, without its ending.
  static func testName(_ fileName: String) -> String {
    let lower = fileName.lowercased()
    let suffix = [testSuffix, ProjectLibrary.blockSuffix].first { lower.hasSuffix($0) } ?? testSuffix
    return String(fileName.dropLast(suffix.count))
  }

  /// The names of the blocks from the community installed in the open project, by id, as its
  /// library reads them.
  static let blockNames = Mutex<[String: String]>([:])

  /// The name of the test or custom block the file at `path` holds; a block installed from the
  /// community is named by its name, or by its id when the open project doesn't have it.
  static func testName(path: String) -> String {
    guard let id = blockPackageId(path: path) else { return testName((path as NSString).lastPathComponent) }
    return blockNames.withLock { $0[id] } ?? id
  }

  /// What the scans of one project read from its test files, so the next scan reads only the
  /// files that changed. Several scans may use it at once.
  final class Cache: @unchecked Sendable {
    fileprivate struct Entry {
      let modified: Date?
      let size: Int?
      let summary: TestFileSummary?
    }

    private let lock = NSLock()
    private var entries: [String: Entry] = [:]
    private var scans = 0
    private var reads = 0
    private var stamp = 0

    /// Scans that went to the end.
    var scanCount: Int { lock.withLock { scans } }
    /// Test files read, over all scans.
    var readCount: Int { lock.withLock { reads } }
    /// Changes each time a scan finds the tests different from the scan before it.
    var changeStamp: Int { lock.withLock { stamp } }

    fileprivate func entry(_ path: String) -> Entry? { lock.withLock { entries[path] } }

    /// Keeps what a whole scan found, in place of what the scans before it did, and returns the
    /// change stamp.
    fileprivate func finish(_ found: [String: Entry], read: Int) -> Int {
      lock.withLock {
        scans += 1
        reads += read
        if read > 0 || found.count != entries.count || !found.keys.allSatisfy({ entries[$0] != nil }) { stamp += 1 }
        entries = found
        return stamp
      }
    }
  }

  /// Returns folders that contain tests (directly or deeper) and the tests themselves.
  /// Hidden items, packages and symbolic links are skipped. Throws if the folder can't be read.
  static func scan(_ root: URL) throws -> [ProjectItem] {
    try scan(root, cache: Cache()).items
  }

  /// Scans as `scan(_:)` does, reading again only the tests whose file changed date or size since
  /// `cache` saw it. `stamp` is the cache's change stamp after the scan: the same as after the
  /// scan before means the same tests.
  static func scan(_ root: URL, cache: Cache) throws -> (items: [ProjectItem], stamp: Int) {
    var files: [Found] = []
    let tree = try walk(root.standardizedFileURL, relativeTo: "", into: &files)
    // Reading and summing up the files is most of a scan: the new and changed ones are read side by side.
    var entries = [String: Cache.Entry](minimumCapacity: files.count)
    var stale: [Int] = []
    for (index, file) in files.enumerated() {
      if let entry = cache.entry(file.url.path), entry.modified == file.modified, entry.size == file.size {
        entries[file.url.path] = entry
      } else {
        stale.append(index)
      }
    }
    let read = stale.map { files[$0] }
    let summaries = read.concurrentMap { (try? String(contentsOf: $0.url, encoding: .utf8)).map { testSummary(json: $0) } }
    try Task.checkCancellation()
    for (file, summary) in zip(read, summaries) {
      entries[file.url.path] = Cache.Entry(modified: file.modified, size: file.size, summary: summary)
    }
    let stamp = cache.finish(entries, read: stale.count)
    return (tree.map { $0.item { entries[$0.path]?.summary } }, stamp)
  }

  /// A test file the walk found, with what tells whether it changed.
  private struct Found {
    let url: URL
    let modified: Date?
    let size: Int?
  }

  /// The folders and test files of a folder, in sidebar order, before the tests are read.
  private struct Tree {
    let id: String
    let name: String
    let url: URL
    let modified: Date?
    /// `nil` for a test.
    let children: [Tree]?
    let testCount: Int

    func item(_ summary: (URL) -> TestFileSummary?) -> ProjectItem {
      guard let children else {
        let summary = summary(url)
        return ProjectItem(
          id: id, name: name, url: url, kind: .test, children: [], testCount: 1, modified: modified,
          keywords: summary?.keywords ?? "", markedFlaky: summary?.flaky ?? false,
          card: summary?.card ?? JEVisionCore.TestCard())
      }
      return ProjectItem(
        id: id, name: name, url: url, kind: .folder, children: children.map { $0.item(summary) }, testCount: testCount,
        modified: nil)
    }
  }

  private static let keys: [URLResourceKey] = [
    .isDirectoryKey, .isRegularFileKey, .isPackageKey, .isSymbolicLinkKey, .contentModificationDateKey, .fileSizeKey,
  ]

  private static func walk(_ folder: URL, relativeTo prefix: String, into files: inout [Found]) throws -> [Tree] {
    try Task.checkCancellation()
    let entries = try FileManager.default.contentsOfDirectory(
      at: folder, includingPropertiesForKeys: keys, options: [.skipsHiddenFiles, .skipsPackageDescendants])
    var folders: [Tree] = []
    var tests: [Tree] = []
    for url in entries {
      guard let values = try? url.resourceValues(forKeys: Set(keys)), values.isSymbolicLink != true else { continue }
      let name = url.lastPathComponent
      let id = prefix.isEmpty ? name : prefix + "/" + name
      if values.isDirectory == true, values.isPackage != true {
        // An unreadable subfolder should not hide the rest of the project.
        guard let children = try? walk(url, relativeTo: id, into: &files) else {
          try Task.checkCancellation()
          continue
        }
        let count = children.reduce(0) { $0 + $1.testCount }
        guard count > 0 else { continue }
        folders.append(Tree(id: id, name: name, url: url, modified: nil, children: children, testCount: count))
      } else if values.isRegularFile == true, isTestFile(name) {
        files.append(Found(url: url, modified: values.contentModificationDate, size: values.fileSize))
        tests.append(
          Tree(id: id, name: testName(name), url: url, modified: values.contentModificationDate, children: nil, testCount: 1))
      }
    }
    let byName: (Tree, Tree) -> Bool = { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    return folders.sorted(by: byName) + tests.sorted(by: byName)
  }

  /// Every test that matches `query`, at any depth, best first: by its file name, then by its
  /// folder, name, description and block texts.
  static func tests(in items: [ProjectItem], matching query: String) -> [ProjectItem] {
    let tests = items.flatMap(\.tests)
    let candidates = tests.map { SearchCandidate(title: $0.name, keywords: $0.searchKeywords) }
    return rankSearch(query: query, candidates: candidates).map { tests[Int($0)] }
  }
}

extension Array where Element: Sendable {
  /// `transform` of each element, in order, worked out side by side on all the cores.
  func concurrentMap<T>(_ transform: @Sendable (Element) -> T) -> [T] {
    let results = UnsafeMutableBufferPointer<T?>.allocate(capacity: count)
    results.initialize(repeating: nil)
    defer {
      results.deinitialize()
      results.deallocate()
    }
    // Each index is written by one iteration only.
    nonisolated(unsafe) let shared = results
    DispatchQueue.concurrentPerform(iterations: count) { shared[$0] = transform(self[$0]) }
    return results.map { $0! }
  }
}
