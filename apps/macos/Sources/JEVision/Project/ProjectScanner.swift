import Foundation
import JEVisionCore

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

  /// Returns folders that contain tests (directly or deeper) and the tests themselves.
  /// Hidden items, packages and symbolic links are skipped. Throws if the folder can't be read.
  static func scan(_ root: URL) throws -> [ProjectItem] {
    try scan(root.standardizedFileURL, relativeTo: "")
  }

  private static let keys: [URLResourceKey] = [
    .isDirectoryKey, .isRegularFileKey, .isPackageKey, .isSymbolicLinkKey, .contentModificationDateKey,
  ]

  private static func scan(_ folder: URL, relativeTo prefix: String) throws -> [ProjectItem] {
    try Task.checkCancellation()
    let entries = try FileManager.default.contentsOfDirectory(
      at: folder, includingPropertiesForKeys: keys, options: [.skipsHiddenFiles, .skipsPackageDescendants])
    var folders: [ProjectItem] = []
    var tests: [ProjectItem] = []
    for url in entries {
      guard let values = try? url.resourceValues(forKeys: Set(keys)), values.isSymbolicLink != true else { continue }
      let name = url.lastPathComponent
      let id = prefix.isEmpty ? name : prefix + "/" + name
      if values.isDirectory == true, values.isPackage != true {
        // An unreadable subfolder should not hide the rest of the project.
        guard let children = try? scan(url, relativeTo: id) else {
          try Task.checkCancellation()
          continue
        }
        let count = children.reduce(0) { $0 + $1.testCount }
        guard count > 0 else { continue }
        folders.append(ProjectItem(
          id: id, name: name, url: url, kind: .folder, children: children, testCount: count, modified: nil))
      } else if values.isRegularFile == true, isTestFile(name) {
        let summary = (try? String(contentsOf: url, encoding: .utf8)).map { testSummary(json: $0) }
        tests.append(ProjectItem(
          id: id, name: testName(name), url: url, kind: .test, children: [], testCount: 1,
          modified: values.contentModificationDate,
          keywords: summary?.keywords ?? "", markedFlaky: summary?.flaky ?? false))
      }
    }
    let byName: (ProjectItem, ProjectItem) -> Bool = { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    return folders.sorted(by: byName) + tests.sorted(by: byName)
  }

  /// Every test that matches `query`, at any depth, best first: by its file name, then by its
  /// folder, name, description and block texts.
  static func tests(in items: [ProjectItem], matching query: String) -> [ProjectItem] {
    let tests = items.flatMap(\.tests)
    let candidates = tests.map { SearchCandidate(title: $0.name, keywords: $0.parentPath + " " + $0.keywords) }
    return rankSearch(query: query, candidates: candidates).map { tests[Int($0)] }
  }
}
