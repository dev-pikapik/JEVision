import CryptoKit
import Foundation

/// Test secrets belong to one project folder. A renamed folder on the same disk keeps its
/// identity; a copied checkout gets a separate one. Paths and secret values stay out of keys.
public struct ProjectSecrets: Sendable {
  public let service: String

  public init(project: URL) {
    let folder = project.standardizedFileURL.resolvingSymlinksInPath()
    let attributes = try? FileManager.default.attributesOfItem(atPath: folder.path)
    let volume = try? folder.resourceValues(forKeys: [.volumeUUIDStringKey]).volumeUUIDString
    let identity: String
    if let inode = attributes?[.systemFileNumber] as? NSNumber,
      let created = attributes?[.creationDate] as? Date,
      let disk = volume ?? (attributes?[.systemNumber] as? NSNumber)?.stringValue
    {
      identity = "folder:\(disk):\(inode):\(created.timeIntervalSince1970.bitPattern)"
    } else {
      identity = "path:\(folder.path)"
    }
    service = "\(SecretStore.tests)/projects/1/" + SHA256.hash(data: Data(identity.utf8)).map { String(format: "%02x", $0) }.joined()
  }

  public func service(environment: String?) -> String {
    environment.map { service + "/environments/" + Data($0.utf8).base64EncodedString() } ?? service
  }

  /// Process environment variables are explicit inputs to this process and keep precedence.
  public func value(_ name: String, environment: String?) -> String? {
    if let value = ProcessInfo.processInfo.environment[name], !value.isEmpty { return value }
    if usesShared(name, environment: environment) { return sharedValue(name, environment: environment) }
    if let environment, let value = SecretStore.keychainValue(name, in: service(environment: environment)) { return value }
    return SecretStore.keychainValue(name, in: service)
  }

  public func isSet(_ name: String, environment: String?) -> Bool {
    if SecretStore.isInEnvironment(name) { return true }
    if usesShared(name, environment: environment) { return hasShared(name, environment: environment) }
    if let environment, SecretStore.isInKeychain(name, in: service(environment: environment)) { return true }
    return SecretStore.isInKeychain(name, in: service)
  }

  public func save(_ value: String, for name: String, environment: String?) throws {
    try SecretStore.save(value, for: name, in: service(environment: environment))
    try setUsesShared(false, for: name, environment: environment)
  }

  public func usesShared(_ name: String, environment: String?) -> Bool {
    if let environment,
      let choice = SecretStore.keychainValue(sharingKey(name), in: service(environment: environment))
    { return choice == "allow" }
    return SecretStore.keychainValue(sharingKey(name), in: service) == "allow"
  }

  public func setUsesShared(_ allowed: Bool, for name: String, environment: String?) throws {
    try SecretStore.save(allowed ? "allow" : "deny", for: sharingKey(name), in: service(environment: environment))
  }

  public func hasShared(_ name: String, environment: String?) -> Bool {
    if let environment, SecretStore.isInKeychain(name, in: SecretStore.service(for: environment)) { return true }
    return SecretStore.isInKeychain(name)
  }

  /// Copies only after the person selects this action; the original shared item stays intact.
  @discardableResult
  public func copyShared(_ name: String, environment: String?) throws -> Bool {
    guard let value = sharedValue(name, environment: environment) else { return false }
    try save(value, for: name, environment: environment)
    return true
  }

  private func sharedValue(_ name: String, environment: String?) -> String? {
    if let environment, let value = SecretStore.keychainValue(name, in: SecretStore.service(for: environment)) { return value }
    return SecretStore.keychainValue(name)
  }

  private func sharingKey(_ name: String) -> String { "sharing/1/" + name }
}
