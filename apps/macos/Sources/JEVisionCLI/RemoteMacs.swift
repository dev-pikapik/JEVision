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
    guard let saved = SecretStore.keychainValue(account, in: service), let data = Data(base64Encoded: saved),
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
      file: URL.applicationSupportDirectory.appending(path: "JEVision/Assistant/macs.json", directoryHint: .notDirectory))
  }

  private struct Entry: Codable {
    var hostId: String
    var name: String
    var deviceId: String
    var relayUrl: String?
    var addrs: [String]
  }

  func all() throws -> [HostRecord] {
    guard let data = try? Data(contentsOf: file) else { return [] }
    return try JSONDecoder().decode([Entry].self, from: data).map {
      HostRecord(hostId: $0.hostId, name: $0.name, deviceId: $0.deviceId, relayUrl: $0.relayUrl, addrs: $0.addrs)
    }
  }

  /// Keeps `mac`, in place of the one with the same id.
  func save(_ mac: HostRecord) throws {
    try write(try all().filter { $0.hostId != mac.hostId } + [mac])
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
    try write(try all().filter { $0.hostId != mac.hostId })
  }

  private func write(_ macs: [HostRecord]) throws {
    let entries = macs.map {
      Entry(hostId: $0.hostId, name: $0.name, deviceId: $0.deviceId, relayUrl: $0.relayUrl, addrs: $0.addrs)
    }
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
    try FileManager.default.createDirectory(
      at: file.deletingLastPathComponent(), withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
    try encoder.encode(entries).write(to: file, options: .atomic)
    try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
  }
}
