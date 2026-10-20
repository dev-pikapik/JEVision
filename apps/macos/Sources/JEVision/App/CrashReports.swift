import Foundation
import MachO
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
    let (frames, unnamed) = crashedFrames(of: crash["callStackTree"] as? [String: Any], symbols: Symbols())
    if !frames.isEmpty {
      lines += ["", "Frames of the thread that crashed, the innermost last:", "```"]
      lines += frames
      lines.append("```")
    }
    if !unnamed.isEmpty {
      lines += ["", "Binaries of the frames without a function name, to look them up in that build's dSYM:"]
      lines += unnamed.map { "- \($0.name) \($0.uuid)" }
    }
    return lines.joined(separator: "\n")
  }

  static let signalNames = [4: "SIGILL", 5: "SIGTRAP", 6: "SIGABRT", 8: "SIGFPE", 10: "SIGBUS", 11: "SIGSEGV"]

  /// The most frames a bug report lists.
  private static let maxFrames = 60

  /// The frames as lines, each with its function when `symbols` knows the binary, and the
  /// binaries whose frames stay a bare offset.
  private static func crashedFrames(of tree: [String: Any]?, symbols: Symbols) -> ([String], [(name: String, uuid: String)]) {
    let stacks = tree?["callStacks"] as? [[String: Any]] ?? []
    let stack = stacks.first { $0["threadAttributed"] as? Bool == true } ?? stacks.first
    var frame = (stack?["callStackRootFrames"] as? [[String: Any]])?.first
    var lines: [String] = []
    var unnamed: [(name: String, uuid: String)] = []
    while let current = frame, lines.count < maxFrames {
      let name = current["binaryName"] as? String ?? "?"
      let address = (current["address"] as? NSNumber).map { "0x" + String($0.uint64Value, radix: 16) } ?? "?"
      let offset = (current["offsetIntoBinaryTextSegment"] as? NSNumber)?.uint64Value
      let uuid = current["binaryUUID"] as? String
      let function = uuid.flatMap { uuid in offset.flatMap { symbols.function(in: uuid, at: $0) } }
      lines.append("\(lines.count)  \(name)  \(address)\(offset.map { " +\($0)" } ?? "")\(function.map { "  \($0)" } ?? "")")
      if function == nil, let uuid, !unnamed.contains(where: { $0.uuid == uuid }) { unnamed.append((name, uuid)) }
      frame = (current["subFrames"] as? [[String: Any]])?.first
    }
    return (lines, unnamed)
  }
}

/// Function names for the frames of a crash, from the binaries this process has loaded: the app's
/// own when it runs the build that crashed, and macOS's libraries when the system is the same. A
/// binary is the same build only when its UUID matches, so a frame is never named after another one.
struct Symbols {
  /// The start of each loaded binary, where its text segment begins, by UUID.
  private var starts: [String: UnsafeRawPointer] = [:]

  init() {
    for index in 0..<_dyld_image_count() {
      guard let header = _dyld_get_image_header(index), let uuid = Self.uuid(of: header) else { continue }
      starts[uuid] = UnsafeRawPointer(header)
    }
  }

  /// "function + 12" for the instruction `offset` bytes into the text segment of the binary with
  /// this UUID, or `nil` when no such binary is loaded or it has no name there.
  func function(in uuid: String, at offset: UInt64) -> String? {
    guard let start = starts[uuid.uppercased()], offset < UInt64(Int.max) else { return nil }
    let address = start + Int(offset)
    var info = Dl_info()
    guard dladdr(address, &info) != 0, UnsafeRawPointer(info.dli_fbase) == start,
      let name = info.dli_sname, let symbol = info.dli_saddr
    else { return nil }
    return "\(Self.readable(String(cString: name))) + \(UnsafeRawPointer(symbol).distance(to: address))"
  }

  /// The UUID the linker gave the binary at `header`, as MetricKit writes it.
  static func uuid(of header: UnsafePointer<mach_header>) -> String? {
    var command = UnsafeRawPointer(header) + MemoryLayout<mach_header_64>.size
    for _ in 0..<header.pointee.ncmds {
      let load = command.load(as: load_command.self)
      if load.cmd == UInt32(LC_UUID) { return UUID(uuid: command.load(as: uuid_command.self).uuid).uuidString }
      command += Int(load.cmdsize)
    }
    return nil
  }

  /// A Swift or Rust symbol as the code names it; C names stay as they are.
  static func readable(_ symbol: String) -> String {
    if let swift = swiftDemangle(symbol, symbol.utf8.count, nil, nil, 0) {
      defer { free(swift) }
      return String(cString: swift)
    }
    return rust(symbol) ?? symbol
  }

  /// `_ZN4core3ptr13drop_in_place17h0123456789abcdefE` → `core::ptr::drop_in_place`: Rust's
  /// mangling, without the hash at the end. `nil` for anything else.
  static func rust(_ symbol: String) -> String? {
    guard symbol.hasPrefix("_ZN"), symbol.hasSuffix("E") else { return nil }
    var rest = symbol.dropFirst(3).dropLast()
    var parts: [String] = []
    while !rest.isEmpty {
      let digits = rest.prefix { $0.isASCII && $0.isNumber }
      guard let length = Int(digits), rest.count >= digits.count + length else { return nil }
      rest = rest.dropFirst(digits.count)
      parts.append(String(rest.prefix(length)))
      rest = rest.dropFirst(length)
    }
    if let hash = parts.last, hash.count == 17, hash.first == "h", hash.dropFirst().allSatisfy(\.isHexDigit) {
      parts.removeLast()
    }
    guard !parts.isEmpty else { return nil }
    let escapes = ["$LT$": "<", "$GT$": ">", "$LP$": "(", "$RP$": ")", "$RF$": "&", "$BP$": "*", "$C$": ",", "$SP$": "@"]
    return parts.map { part in
      let plain = (part.hasPrefix("_$") ? String(part.dropFirst()) : part).replacingOccurrences(of: "..", with: "::")
      return escapes.reduce(plain) { $0.replacingOccurrences(of: $1.key, with: $1.value) }
        .replacing(/\$u([0-9a-f]{2,6})\$/) { match in
          UInt32(match.output.1, radix: 16).flatMap(Unicode.Scalar.init).map { String($0) } ?? String(match.output.0)
        }
    }.joined(separator: "::")
  }
}

/// The Swift runtime's demangler: a string to free, or `nil` when the name isn't Swift's.
@_silgen_name("swift_demangle")
private func swiftDemangle(
  _ name: UnsafePointer<CChar>, _ length: Int, _ buffer: UnsafeMutablePointer<CChar>?,
  _ size: UnsafeMutablePointer<Int>?, _ flags: UInt32
) -> UnsafeMutablePointer<CChar>?

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
