import Foundation

/// Before an update that stores tests or settings in a newer way, saves a copy of them, so they
/// can still be opened with the version from before. A copy is never deleted by JEVision.
///
/// The feed tells the data format of each version (`jevision:dataFormat`), the app its own
/// (`JEVisionDataFormat` in Info.plist). Raise both when a version changes what it writes.
struct UpdateDataGuard {
  static let feedKey = "jevision:dataFormat"
  static let infoKey = "JEVisionDataFormat"
  static let folder = URL.applicationSupportDirectory.appending(path: "JEVision", directoryHint: .isDirectory)
  static var backups: URL { folder.appending(path: "Backups", directoryHint: .isDirectory) }

  var current = Bundle.main.object(forInfoDictionaryKey: infoKey) as? Int ?? 1
  var folder = Self.folder
  var settings = Bundle.main.bundleIdentifier.flatMap { UserDefaults.standard.persistentDomain(forName: $0) }
  var project = Self.openProject(in: .standard)
  var now = Date.now

  var backups: URL { folder.appending(path: "Backups", directoryHint: .isDirectory) }

  /// Whether a version with `dataFormat` from the feed stores data in a newer way than this one.
  static func needsBackup(current: Int, dataFormat: String?) -> Bool {
    guard let format = dataFormat.flatMap({ Int($0.trimmingCharacters(in: .whitespacesAndNewlines)) }) else { return false }
    return format > current
  }

  /// Copies JEVision's folder in Application Support, the settings and the open project into
  /// `Backups/before-<version>-<date>`, when `dataFormat` is newer. Returns that folder, or nil when
  /// no copy is needed. Throws, and leaves no half-made copy behind, when anything can't be copied.
  func prepare(version: String, dataFormat: String?) throws -> URL? {
    guard Self.needsBackup(current: current, dataFormat: dataFormat) else { return nil }
    let day = Date.ISO8601FormatStyle(timeZone: .current).year().month().day().format(now)
    let target = backups.appending(path: "before-\(version)-\(day)", directoryHint: .isDirectory)
    let files = FileManager.default
    guard !files.fileExists(atPath: target.path) else { return target }
    let partial = backups.appending(path: ".\(target.lastPathComponent).partial", directoryHint: .isDirectory)
    do {
      if files.fileExists(atPath: partial.path) { try files.removeItem(at: partial) }
      try files.createDirectory(at: partial, withIntermediateDirectories: true)
      try copy(folder, to: partial.appending(path: "Application Support"), skipping: ["Backups"])
      if let settings {
        try PropertyListSerialization.data(fromPropertyList: settings, format: .xml, options: 0)
          .write(to: partial.appending(path: "Settings.plist"))
      }
      if let project {
        // The history of the project is in git, and runs with their videos aren't rewritten.
        try copy(project, to: partial.appending(path: "Project/\(project.lastPathComponent)"), skipping: [".git", ".jevision"])
      }
      try files.moveItem(at: partial, to: target)
      return target
    } catch {
      try? files.removeItem(at: partial)
      throw BackupFailed(version: version, reason: error.localizedDescription)
    }
  }

  /// The project folder JEVision opens at launch, from its bookmark.
  static func openProject(in defaults: UserDefaults) -> URL? {
    guard let data = defaults.data(forKey: ProjectStore.Key.bookmark) else { return nil }
    var stale = false
    let url = try? URL(resolvingBookmarkData: data, options: [.withoutUI, .withoutMounting], bookmarkDataIsStale: &stale)
    return url.flatMap { FileManager.default.fileExists(atPath: $0.path) ? $0 : nil }
  }

  private func copy(_ source: URL, to target: URL, skipping: Set<String>) throws {
    let files = FileManager.default
    guard files.fileExists(atPath: source.path) else { return }
    try files.createDirectory(at: target, withIntermediateDirectories: true)
    for item in try files.contentsOfDirectory(at: source, includingPropertiesForKeys: nil)
    where !skipping.contains(item.lastPathComponent) {
      try files.copyItem(at: item, to: target.appending(path: item.lastPathComponent))
    }
  }

  struct BackupFailed: LocalizedError {
    let version: String
    let reason: String

    var errorDescription: String? {
      String(
        localized: "JEVision \(version) wasn’t installed. It stores tests and settings in a new way, and JEVision couldn’t save a copy of yours first: \(reason)")
    }
  }
}
