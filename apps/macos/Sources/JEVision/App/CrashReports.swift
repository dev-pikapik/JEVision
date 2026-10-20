import Foundation
import MetricKit

/// Crash reports of JEVision itself, kept on this Mac in the app's folder. They are saved only
/// after the person turned collecting on, and nothing sends them anywhere: they leave the Mac only
/// when the person adds one to a bug report.
struct CrashReports: Sendable {
  static let enabledKey = "collectCrashReports"
  /// The reports kept; older ones are deleted when a new one comes.
  static let kept = 20

  let folder: URL

  init(folder: URL = URL.applicationSupportDirectory.appending(path: "JEVision/Crash Reports", directoryHint: .isDirectory)) {
    self.folder = folder
  }

  static var isEnabled: Bool { UserDefaults.standard.bool(forKey: enabledKey) }

  /// One saved crash report.
  struct Saved: Identifiable, Equatable {
    let url: URL
    let date: Date
    var id: URL { url }
  }

  /// Saves what MetricKit delivered about a crash that ended at `date`, and deletes the oldest
  /// reports beyond `kept`.
  @discardableResult
  func save(_ payload: Data, crashedAt date: Date) throws -> URL {
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let stamp = date.formatted(.iso8601).replacingOccurrences(of: ":", with: "-")
    var url = folder.appending(path: "crash-\(stamp).json", directoryHint: .notDirectory)
    var copy = 1
    while FileManager.default.fileExists(atPath: url.path) {
      copy += 1
      url = folder.appending(path: "crash-\(stamp)-\(copy).json", directoryHint: .notDirectory)
    }
    try payload.write(to: url, options: .atomic)
    // The file's date is when the app crashed: it orders the list and is the time people see.
    try FileManager.default.setAttributes([.modificationDate: date], ofItemAtPath: url.path)
    for old in all().dropFirst(Self.kept) { try? FileManager.default.removeItem(at: old.url) }
    return url
  }

  /// The saved reports, the newest first.
  func all() -> [Saved] {
    let files = (try? FileManager.default.contentsOfDirectory(
      at: folder, includingPropertiesForKeys: [.contentModificationDateKey], options: .skipsHiddenFiles)) ?? []
    return files.filter { $0.pathExtension == "json" && $0.lastPathComponent.hasPrefix("crash-") }
      .map { Saved(url: $0, date: (try? $0.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast) }
      .sorted { ($0.date, $0.url.lastPathComponent) > ($1.date, $1.url.lastPathComponent) }
  }

  /// A crash report as text for a bug report: when, which build, which signal and the frames of
  /// the thread that crashed. `nil` when the file holds no crash.
  static func summary(of payload: Data) -> String? {
    guard let root = try? JSONSerialization.jsonObject(with: payload) as? [String: Any],
      let crash = (root["crashDiagnostics"] as? [[String: Any]])?.first
    else { return nil }
    let meta = crash["diagnosticMetaData"] as? [String: Any] ?? [:]
    func text(_ key: String) -> String? { (meta[key] as? CustomStringConvertible).map(\.description) }
    var lines = ["### Crash report"]
    if let end = root["timeStampEnd"] as? String { lines.append("- Time: \(end)") }
    let version = [text("appVersion"), text("appBuildVersion").map { "(\($0))" }].compactMap { $0 }.joined(separator: " ")
    if !version.isEmpty { lines.append("- JEVision \(version)") }
    let system = [text("osVersion"), text("deviceType"), text("platformArchitecture")].compactMap { $0 }.joined(separator: ", ")
    if !system.isEmpty { lines.append("- System: \(system)") }
    if let signal = (meta["signal"] as? Int) { lines.append("- Signal: \(signal)\(signalNames[signal].map { " (\($0))" } ?? "")") }
    if let type = text("exceptionType") { lines.append("- Exception type: \(type), code \(text("exceptionCode") ?? "–")") }
    if let reason = text("terminationReason"), !reason.isEmpty { lines.append("- Reason: \(reason)") }
    let frames = crashedFrames(of: crash["callStackTree"] as? [String: Any])
    if !frames.isEmpty {
      lines += ["", "Frames of the thread that crashed, the innermost last:", "```"]
      lines += frames
      lines.append("```")
    }
    return lines.joined(separator: "\n")
  }

  static let signalNames = [4: "SIGILL", 5: "SIGTRAP", 6: "SIGABRT", 8: "SIGFPE", 10: "SIGBUS", 11: "SIGSEGV"]

  /// The most frames a bug report lists.
  private static let maxFrames = 60

  private static func crashedFrames(of tree: [String: Any]?) -> [String] {
    let stacks = tree?["callStacks"] as? [[String: Any]] ?? []
    let stack = stacks.first { $0["threadAttributed"] as? Bool == true } ?? stacks.first
    var frame = (stack?["callStackRootFrames"] as? [[String: Any]])?.first
    var lines: [String] = []
    while let current = frame, lines.count < maxFrames {
      let name = current["binaryName"] as? String ?? "?"
      let address = (current["address"] as? NSNumber).map { "0x" + String($0.uint64Value, radix: 16) } ?? "?"
      let offset = (current["offsetIntoBinaryTextSegment"] as? NSNumber).map { " +\($0.uint64Value)" } ?? ""
      lines.append("\(lines.count)  \(name)  \(address)\(offset)")
      frame = (current["subFrames"] as? [[String: Any]])?.first
    }
    return lines
  }
}

/// Saves the crash reports MetricKit hands over, once the person has agreed. macOS delivers the
/// diagnostics of a crash at the next launch, up to a day later.
final class CrashCollector: NSObject, MXMetricManagerSubscriber, Sendable {
  static let shared = CrashCollector()

  /// Starts or stops listening; follows the setting.
  static func apply(enabled: Bool) {
    if enabled {
      MXMetricManager.shared.add(shared)
    } else {
      MXMetricManager.shared.remove(shared)
    }
  }

  func didReceive(_ payloads: [MXDiagnosticPayload]) {
    for payload in payloads where payload.crashDiagnostics?.isEmpty == false {
      do {
        try CrashReports().save(payload.jsonRepresentation(), crashedAt: payload.timeStampEnd)
        AppLog.info("A crash report was saved")
      } catch {
        AppLog.error("A crash report couldn’t be saved: \(error.localizedDescription)")
      }
    }
  }
}
