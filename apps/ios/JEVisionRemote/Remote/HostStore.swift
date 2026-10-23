import Foundation
import func JEVisionMobile.appVersion

/// Paired Macs on disk, in Application Support: `{"format":1,"macs":[…]}`. The records hold no
/// secrets: a Mac trusts this iPhone only by the key in the Secure Enclave. What a newer version
/// wrote and this one doesn't know stays in the file when it saves.
struct HostStore: Sendable {
  /// The list was saved by a newer JEVision Remote. This version reads it but never writes it.
  struct NewerList: Error {}

  /// The file is there but couldn't be opened, as before the iPhone's first unlock after a
  /// restart. Nothing is written over it: the Macs in it would be lost.
  struct NotOpened: Error {}

  enum LoadProblem {
    /// The file couldn't be read at all and was moved to `unreadableURL`.
    case unreadable
    /// The file couldn't be opened; nothing may be saved until it loads.
    case notOpened
  }

  let url: URL

  static let standard = HostStore(
    url: URL.applicationSupportDirectory.appending(path: "PairedMacs.json"))

  /// Where a file that couldn't be read at all is kept: `PairedMacs.unreadable.json`.
  var unreadableURL: URL {
    url.deletingLastPathComponent().appending(path: url.deletingPathExtension().lastPathComponent + ".unreadable.json")
  }

  /// The paired Macs this version can read, and why the list starts empty if it does.
  func load() -> (hosts: [HostRecord], problem: LoadProblem?) {
    do {
      let file = try read()
      return (file.macs.compactMap(Self.record), file.wasUnreadable ? .unreadable : nil)
    } catch is NotOpened {
      return ([], .notOpened)
    } catch {
      // It couldn't be moved aside and stays where it is; `save` moves it first.
      return ([], .unreadable)
    }
  }

  /// Writes `hosts` over what the file holds: keys this version doesn't know stay, Macs that
  /// aren't in `hosts` go, entries this version can't read stay as they were.
  func save(_ hosts: [HostRecord]) throws {
    let file = try read()
    if file.isNewer { throw NewerList() }
    var stored: [String: [String: Any]] = [:]
    for case let mac as [String: Any] in file.macs {
      if let id = mac["hostId"] as? String { stored[id] = mac }
    }
    var macs: [Any] = try hosts.map { host in
      var mac = stored[host.hostId] ?? [:]
      mac["relayUrl"] = nil
      let known = try JSONSerialization.jsonObject(with: JSONEncoder().encode(host)) as? [String: Any] ?? [:]
      return mac.merging(known) { $1 }
    }
    let saved = Set(hosts.map(\.hostId))
    macs += file.macs.filter { mac in
      Self.record(mac) == nil && !saved.contains((mac as? [String: Any])?["hostId"] as? String ?? "")
    }
    var top = file.top
    top["format"] = 1
    top["macs"] = macs
    let data = try JSONSerialization.data(withJSONObject: top, options: [.sortedKeys, .withoutEscapingSlashes])
    try FileManager.default.createDirectory(
      at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
    try data.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
  }

  private struct File {
    /// Every top-level key as saved.
    var top: [String: Any] = [:]
    var macs: [Any] = []
    var isNewer = false
    var wasUnreadable = false
  }

  /// What the file holds now; empty when there is none yet. A file this version can't read at
  /// all is moved to `unreadableURL` and reads as empty.
  private func read() throws -> File {
    let data: Data
    do {
      data = try Data(contentsOf: url)
    } catch let error as CocoaError where error.code == .fileReadNoSuchFile {
      return File()
    } catch {
      throw NotOpened()
    }
    if let file = Self.file(data) { return file }
    try? FileManager.default.removeItem(at: unreadableURL)
    try FileManager.default.moveItem(at: url, to: unreadableURL)
    return File(wasUnreadable: true)
  }

  /// Reads the file by the Mac's rules for its own list: `nil` when this version can't read it.
  private static func file(_ data: Data) -> File? {
    switch try? JSONSerialization.jsonObject(with: data) {
    case let macs as [Any]:
      // Saved by the first versions, before the list had a format.
      return File(macs: macs)
    case let top as [String: Any]:
      guard let format = format(top["format"]) else { return nil }
      let isNewer = format > 1 || needsNewer(top["minVersion"])
      // A newer list may keep its Macs elsewhere; it is only read, never written.
      guard let macs = top["macs"] as? [Any] ?? (isNewer ? [] : nil) else { return nil }
      return File(top: top, macs: macs, isNewer: isNewer)
    default:
      return nil
    }
  }

  /// `format` as a whole number above zero; 1 when it's missing. `nil` for `2.5`, `"2"` or `true`.
  private static func format(_ value: Any?) -> Int? {
    guard let value else { return 1 }
    guard let number = value as? NSNumber, CFGetTypeID(number) == CFNumberGetTypeID(),
      !CFNumberIsFloatType(number), number.int64Value > 0
    else { return nil }
    return number.intValue
  }

  /// Whether `minVersion` is later than this app. One this version can't read counts as later,
  /// so the file is left alone.
  private static func needsNewer(_ minVersion: Any?) -> Bool {
    guard let minVersion else { return false }
    guard let min = (minVersion as? String).flatMap(Version.init), let own = Version(appVersion())
    else { return true }
    return min > own
  }

  private static func record(_ mac: Any) -> HostRecord? {
    guard let mac = mac as? [String: Any], let data = try? JSONSerialization.data(withJSONObject: mac) else { return nil }
    return try? JSONDecoder().decode(HostRecord.self, from: data)
  }

  /// A JEVision version read and ordered as the core does: `MAJOR.MINOR.PATCH` with an optional
  /// `-pre.release` part. `1.2.0` is later than `1.2.0-beta.2` and earlier than `1.10.0`; a
  /// `+build` part is ignored.
  struct Version: Comparable {
    let numbers: [UInt64]
    let pre: [Substring]

    init?(_ text: String) {
      let text = text.prefix { $0 != "+" }
      let dash = text.firstIndex(of: "-")
      let numbers = text[..<(dash ?? text.endIndex)].split(separator: ".", omittingEmptySubsequences: false)
      let pre = dash.map { text[text.index(after: $0)...].split(separator: ".", omittingEmptySubsequences: false) } ?? []
      let values = numbers.compactMap { Self.isDigits($0) && ($0 == "0" || $0.first != "0") ? UInt64($0) : nil }
      guard numbers.count == 3, values.count == 3,
        pre.allSatisfy({ part in
          !part.isEmpty && part.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-") }
            && !(part.count > 1 && part.first == "0" && Self.isDigits(part))
        })
      else { return nil }
      self.numbers = values
      self.pre = pre
    }

    static func < (a: Self, b: Self) -> Bool {
      if a.numbers != b.numbers { return a.numbers.lexicographicallyPrecedes(b.numbers) }
      // A pre-release comes before the release itself.
      switch (a.pre.isEmpty, b.pre.isEmpty) {
      case (true, _): return false
      case (false, true): return true
      case (false, false):
        for (x, y) in zip(a.pre, b.pre) where x != y {
          // Numbers compare as numbers and come before words.
          switch (isDigits(x) ? UInt64(x) : nil, isDigits(y) ? UInt64(y) : nil) {
          case let (x?, y?): return x < y
          case (.some, nil): return true
          case (nil, .some): return false
          case (nil, nil): return x < y
          }
        }
        return a.pre.count < b.pre.count
      }
    }

    private static func isDigits(_ text: Substring) -> Bool {
      !text.isEmpty && text.allSatisfy { $0.isASCII && $0.isNumber }
    }
  }
}
