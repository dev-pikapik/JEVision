import CryptoKit
import Foundation
import JEVisionCore
import JEVisionHands

/// This computer's key for the Macs its assistants use: a P-256 key kept in the Keychain. A Mac
/// knows the computer by it and lets it in only when it proves it has the key.
final class ComputerKey: AssistantKey, @unchecked Sendable {
  private static let service = "JEVision Assistant"
  private static let account = "computer-key"

  private let key: P256.Signing.PrivateKey

  init(_ key: P256.Signing.PrivateKey) {
    self.key = key
  }

  /// The key from the Keychain, or nil when this computer has none yet.
  static func existing() -> ComputerKey? {
    guard let saved = SecretStore.keychainValue(account, in: service),
      let data = Data(base64Encoded: saved),
      let key = try? P256.Signing.PrivateKey(rawRepresentation: data)
    else { return nil }
    return ComputerKey(key)
  }

  /// The key from the Keychain, made and saved the first time.
  static func existingOrNew() throws -> ComputerKey {
    if let key = existing() { return key }
    let key = P256.Signing.PrivateKey()
    try SecretStore.save(key.rawRepresentation.base64EncodedString(), for: account, in: service)
    return ComputerKey(key)
  }

  /// Deletes the key: no Mac knows this computer after that.
  static func remove() throws {
    try SecretStore.remove(account, in: service)
  }

  func publicKey() -> Data {
    key.publicKey.x963Representation
  }

  func sign(message: Data) -> Data? {
    try? key.signature(for: message).derRepresentation
  }
}

/// The Macs this computer is paired with: their names and where to reach them, in a file in the
/// user's Library. Nothing secret is in it; the key is in the Keychain.
struct RemoteMacs {
  let file: URL

  static var standard: RemoteMacs {
    RemoteMacs(
      file: Profile.current.supportFolder.appending(
        path: "Assistant/macs.json", directoryHint: .notDirectory))
  }

  private struct Entry: Codable {
    var hostId: String
    var name: String
    var deviceId: String
    var relayUrl: String?
    var addrs: [String]
  }

  enum StorageError: LocalizedError {
    case newer, damaged

    var errorDescription: String? {
      switch self {
      case .newer: "Update JEVision before changing this list of paired Macs."
      case .damaged: "The list of paired Macs could not be read. The saved file has been kept."
      }
    }
  }

  private struct File {
    var top: [String: Any] = [:]
    var macs: [Any] = []
    var newer = false
    var legacy: Data?
  }

  func all() throws -> [HostRecord] {
    try read().macs.compactMap(Self.record)
  }

  /// Keeps `mac`, in place of the one with the same id, retaining fields it doesn't change.
  func save(_ mac: HostRecord) throws {
    var saved = try read()
    guard !saved.newer else { throw StorageError.newer }
    let old =
      saved.macs.first { ($0 as? [String: Any])?["hostId"] as? String == mac.hostId }
      as? [String: Any] ?? [:]
    var entry = old
    entry["hostId"] = mac.hostId
    entry["name"] = mac.name
    entry["deviceId"] = mac.deviceId
    entry["relayUrl"] = mac.relayUrl
    entry["addrs"] = mac.addrs
    saved.macs.removeAll { ($0 as? [String: Any])?["hostId"] as? String == mac.hostId }
    saved.macs.append(entry)
    try write(saved)
  }

  /// The Macs `name` can mean: the name, whatever the case, or the start of the id.
  func matching(_ name: String) throws -> [HostRecord] {
    let wanted = name.trimmingCharacters(in: .whitespaces)
    return try all().filter {
      $0.name.compare(wanted, options: [.caseInsensitive, .diacriticInsensitive]) == .orderedSame
        || (wanted.count >= 6 && $0.hostId.hasPrefix(wanted.lowercased()))
    }
  }

  func remove(_ mac: HostRecord) throws {
    var saved = try read()
    guard !saved.newer else { throw StorageError.newer }
    saved.macs.removeAll { ($0 as? [String: Any])?["hostId"] as? String == mac.hostId }
    try write(saved)
  }

  private static func record(_ value: Any) -> HostRecord? {
    guard let entry = value as? [String: Any],
      let data = try? JSONSerialization.data(withJSONObject: entry),
      let parsed = try? JSONDecoder().decode(Entry.self, from: data)
    else { return nil }
    return HostRecord(
      hostId: parsed.hostId, name: parsed.name, deviceId: parsed.deviceId,
      relayUrl: parsed.relayUrl, addrs: parsed.addrs)
  }

  /// Missing files start empty. Unreadable files and newer data never become an empty writable list.
  private func read() throws -> File {
    let data: Data
    do { data = try Data(contentsOf: file) } catch CocoaError.fileReadNoSuchFile { return File() }
    let json = try JSONSerialization.jsonObject(with: data)
    if let macs = json as? [Any] { return File(macs: macs, legacy: data) }
    guard let top = json as? [String: Any] else { throw StorageError.damaged }
    var format = 1
    if let value = top["format"] {
      guard let number = value as? NSNumber, CFGetTypeID(number) == CFNumberGetTypeID(),
        !CFNumberIsFloatType(number), number.int64Value > 0
      else { throw StorageError.damaged }
      format = number.intValue
    }
    let newer = format > 1 || Self.needsNewer(top["minVersion"])
    guard let macs = top["macs"] as? [Any] ?? (newer ? [] : nil) else { throw StorageError.damaged }
    return File(top: top, macs: macs, newer: newer)
  }

  private static func needsNewer(_ value: Any?) -> Bool {
    guard let value else { return false }
    guard let text = value as? String, let required = Version(text),
      let own = Version(jevisionVersion())
    else { return true }
    return required > own
  }

  private func write(_ saved: File) throws {
    guard !saved.newer else { throw StorageError.newer }
    var top = saved.top
    top["format"] = 1
    top["macs"] = saved.macs
    let data = try JSONSerialization.data(
      withJSONObject: top, options: [.prettyPrinted, .sortedKeys])
    let files = FileManager.default
    try files.createDirectory(
      at: file.deletingLastPathComponent(), withIntermediateDirectories: true,
      attributes: [.posixPermissions: 0o700])
    if let legacy = saved.legacy {
      let backup = file.deletingPathExtension().appendingPathExtension("v0.json")
      if !files.fileExists(atPath: backup.path) {
        try legacy.write(to: backup, options: .withoutOverwriting)
        try files.setAttributes([.posixPermissions: 0o600], ofItemAtPath: backup.path)
      }
    }
    try data.write(to: file, options: .atomic)
    try files.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
  }

  struct Version: Comparable {
    let numbers: [UInt64]
    let pre: [Substring]

    init?(_ text: String) {
      let text = text.prefix { $0 != "+" }
      let dash = text.firstIndex(of: "-")
      let numbers = text[..<(dash ?? text.endIndex)].split(
        separator: ".", omittingEmptySubsequences: false)
      let pre =
        dash.map {
          text[text.index(after: $0)...].split(separator: ".", omittingEmptySubsequences: false)
        } ?? []
      let values = numbers.compactMap {
        Self.isDigits($0) && ($0 == "0" || $0.first != "0") ? UInt64($0) : nil
      }
      guard numbers.count == 3, values.count == 3,
        pre.allSatisfy({ part in
          !part.isEmpty
            && part.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-") }
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
          case (let x?, let y?): return x < y
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
