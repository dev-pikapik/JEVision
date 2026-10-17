import Foundation

/// Finds test files in a project folder. Only names and folders matter here;
/// reading what is inside a test is the core's job.
enum ProjectScanner {
  static let testSuffix = ".flow.json"

  static func isTestFile(_ fileName: String) -> Bool {
    fileName.count > testSuffix.count && fileName.lowercased().hasSuffix(testSuffix)
  }

  static func testName(_ fileName: String) -> String {
    String(fileName.dropLast(testSuffix.count))
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
        tests.append(ProjectItem(
          id: id, name: testName(name), url: url, kind: .test, children: [], testCount: 1,
          modified: values.contentModificationDate))
      }
    }
    let byName: (ProjectItem, ProjectItem) -> Bool = { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    return folders.sorted(by: byName) + tests.sorted(by: byName)
  }

  /// Every test whose name contains `query`, at any depth, in sidebar order.
  static func tests(in items: [ProjectItem], matching query: String) -> [ProjectItem] {
    let query = query.trimmingCharacters(in: .whitespaces)
    return items.flatMap { item in
      switch item.kind {
      case .test: item.name.localizedStandardContains(query) ? [item] : []
      case .folder: tests(in: item.children, matching: query)
      }
    }
  }
}
