import Foundation
import JEVisionCore
import JEVisionHands
import OSLog
import Synchronization

/// How much the app log says. Each level includes the ones before it.
enum LogLevel: String, CaseIterable, Comparable {
  /// What went wrong.
  case error
  /// Also what the app did: the project opened, a run started and how it ended.
  case info
  /// Also every step of a run.
  case debug

  static let `default` = LogLevel.info

  static func < (a: LogLevel, b: LogLevel) -> Bool {
    allCases.firstIndex(of: a)! < allCases.firstIndex(of: b)!
  }
}

/// A log file that starts a new one when it is full and keeps only the last few, so the log never
/// takes more than `maxBytes * files` of the disk. `JEVision.log` is the newest, `JEVision.1.log`
/// the one before it, and so on.
struct RotatingLog: Sendable {
  let folder: URL
  var name = "JEVision"
  var maxBytes = 512 * 1024
  var files = 4

  func url(_ index: Int) -> URL {
    folder.appending(path: index == 0 ? "\(name).log" : "\(name).\(index).log", directoryHint: .notDirectory)
  }

  func append(_ line: String) throws {
    let data = Data((line + "\n").utf8)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let current = url(0)
    let size = (try? current.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
    if size > 0, size + data.count > maxBytes { try rotate() }
    if !FileManager.default.fileExists(atPath: current.path) {
      FileManager.default.createFile(atPath: current.path, contents: nil)
    }
    let handle = try FileHandle(forWritingTo: current)
    defer { try? handle.close() }
    try handle.seekToEnd()
    try handle.write(contentsOf: data)
  }

  /// The newest whole lines, the oldest of them first, `limit` characters at most. They come from
  /// the files before the newest too when it has only just started.
  func tail(limit: Int) -> [String] {
    var lines: [String] = []
    var count = 0
    for index in 0..<files {
      guard let text = try? String(contentsOf: url(index), encoding: .utf8) else { break }
      for line in text.split(whereSeparator: \.isNewline).reversed() {
        count += line.count + 1
        guard count <= limit else { return lines.reversed() }
        lines.append(String(line))
      }
    }
    return lines.reversed()
  }

  private func rotate() throws {
    let manager = FileManager.default
    try? manager.removeItem(at: url(files - 1))
    for index in stride(from: files - 2, through: 0, by: -1) where manager.fileExists(atPath: url(index).path) {
      try manager.moveItem(at: url(index), to: url(index + 1))
    }
  }
}

/// The app's log: one line per event in `~/Library/Logs/JEVision`, for a person who wants to know
/// what happened or to attach it to a bug report. Nothing here is sent anywhere.
///
/// Log what the app did and which error came back, never what a test typed or a secret's value.
/// As a second line of defence every message goes through `LogRedaction` first.
enum AppLog {
  static let levelKey = "logLevel"
  static let folder = Profile.current.logsFolder

  private static let file = Mutex(RotatingLog(folder: folder))
  private static let maxLine = 2000

  static var level: LogLevel {
    get { Profile.current.defaults.string(forKey: levelKey).flatMap(LogLevel.init) ?? .default }
    set { Profile.current.defaults.set(newValue.rawValue, forKey: levelKey) }
  }

  static func error(_ message: @autoclosure () -> String, _ category: String = "app") {
    write(.error, category, message)
  }

  static func info(_ message: @autoclosure () -> String, _ category: String = "app") {
    write(.info, category, message)
  }

  static func debug(_ message: @autoclosure () -> String, _ category: String = "app") {
    write(.debug, category, message)
  }

  private static func write(_ level: LogLevel, _ category: String, _ message: () -> String) {
    guard level <= self.level else { return }
    let text = LogRedaction.mask(message())
    // Dynamic text is private in the system log: Console shows it only on this Mac, to this user.
    let logger = Logger(subsystem: Bundle.main.bundleIdentifier ?? "JEVision", category: category)
    switch level {
    case .error: logger.error("\(text)")
    case .info: logger.info("\(text)")
    case .debug: logger.debug("\(text)")
    }
    let line = format(level, category, text, at: .now)
    // A log that can't be written is no reason to disturb what the person is doing.
    try? file.withLock { try $0.append(line) }
  }

  /// The newest lines of the log for a bug report, when the person chooses to add them: whole
  /// lines up to `limit` characters, so the report stays within what GitHub takes, and masked once
  /// more in case a line was written before a rule for its secret existed.
  static func tail(limit: Int = 20_000) -> [String] {
    file.withLock { $0.tail(limit: limit) }.map(LogRedaction.mask)
  }

  static func format(_ level: LogLevel, _ category: String, _ message: String, at date: Date) -> String {
    let stamp = date.formatted(Date.ISO8601FormatStyle(includingFractionalSeconds: true))
    let flat = message.split(whereSeparator: \.isNewline).joined(separator: " / ")
    let text = flat.count > maxLine ? String(flat.prefix(maxLine)) + "…" : flat
    return "\(stamp) \(level.rawValue.uppercased().padding(toLength: 5, withPad: " ", startingAt: 0)) [\(category)] \(text)"
  }

  /// What the log says first on every launch: which build and which Mac, for whoever reads it.
  static func started() {
    let info = Bundle.main.infoDictionary
    let version = info?["CFBundleShortVersionString"] as? String ?? "?"
    let build = info?["CFBundleVersion"] as? String ?? "?"
    let os = ProcessInfo.processInfo.operatingSystemVersion
    AppLog.info("JEVision \(version) (\(build)) started on macOS \(os.majorVersion).\(os.minorVersion).\(os.patchVersion), log level \(level.rawValue)")
  }

  /// A run began.
  static func runStarted(_ test: URL, project: URL) {
    info("Run started: \(relative(test, in: project))", "run")
  }

  /// A run ended, with its verdict, or without a report.
  static func runEnded(_ test: URL, project: URL, outcome: RunOutcome) {
    switch outcome {
    case .finished(_, _, let report):
      info("Run ended: \(relative(test, in: project)), \(report.verdict)", "run")
    case .failed(let reason):
      error("Run couldn’t finish: \(relative(test, in: project)): \(reason)", "run")
    }
  }

  /// A step of a run ended.
  static func step(_ step: Step) {
    debug("Step \(step.seq) \(step.block) (\(step.blockType)): \(step.status)", "run")
  }

  private static func relative(_ file: URL, in project: URL) -> String {
    let root = project.standardizedFileURL.path
    let path = file.standardizedFileURL.path
    return path.hasPrefix(root + "/") ? String(path.dropFirst(root.count + 1)) : file.lastPathComponent
  }
}

/// Hides what looks like a secret in a line of the log: keys, tokens and passwords in addresses,
/// headers and messages that came back from a server.
enum LogRedaction {
  static let placeholder = "••••"

  /// Each pattern with what replaces a match; `$1` and `$2` keep the words around the secret.
  private static let rules: [(NSRegularExpression, String)] = [
    // https://user:password@host
    (#"(://)[^/\s:@]+:[^/\s@]+@"#, "$1\(placeholder)@"),
    // ?key=…, &api_key=…, token=…, password=…
    (#"(?i)(\b[\w-]*(?:key|token|secret|password|passwd|signature|sig)=)[^&\s"']+"#, "$1\(placeholder)"),
    // Authorization: Bearer …, "x-api-key": "…"
    (#"(?i)(\b(?:proxy-)?authorization\b|\bx-api-key\b|\bapi-key\b)(["']?\s*[:=]\s*["']?)(?:(?:bearer|basic|token)\s+)?[^\s"',;]+"#, "$1$2\(placeholder)"),
    (#"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]{8,}"#, "$1 \(placeholder)"),
    // Keys of AI providers and GitHub, Slack and Google tokens.
    (#"\b(?:sk|pk|rk)-[A-Za-z0-9_-]{16,}"#, placeholder),
    (#"\bAIza[0-9A-Za-z_-]{30,}"#, placeholder),
    (#"\bgh[pousr]_[A-Za-z0-9]{30,}"#, placeholder),
    (#"\bgithub_pat_[A-Za-z0-9_]{30,}"#, placeholder),
    (#"\bxox[abprs]-[A-Za-z0-9-]{10,}"#, placeholder),
    // Telegram bot tokens sit in the address; Slack webhook addresses are secrets themselves.
    (#"\b(bot)\d{5,}:[A-Za-z0-9_-]{20,}"#, "$1\(placeholder)"),
    (#"(hooks\.slack\.com/services/)[A-Za-z0-9/]+"#, "$1\(placeholder)"),
  ].map { (try! NSRegularExpression(pattern: $0.0), $0.1) }

  static func mask(_ text: String) -> String {
    rules.reduce(text) { text, rule in
      rule.0.stringByReplacingMatches(
        in: text, range: NSRange(text.startIndex..., in: text), withTemplate: rule.1)
    }
  }
}
