import Foundation
import JEVisionCore
import Security

/// Secret values live in the environment or in the Keychain, never in a test file.
/// Only a run reads a value back, to type it; nothing shows one.
public enum SecretStore {
  /// Keychain service of the secrets tests type.
  public static let tests = "JEVision"

  public static func isSet(_ name: String, in service: String = tests) -> Bool {
    isInEnvironment(name) || isInKeychain(name, in: service)
  }

  /// Keychain service of the secrets tests type in `environment`; `nil` for the ones every
  /// environment shares.
  public static func service(for environment: String?) -> String {
    environment.map { "\(tests)/\($0)" } ?? tests
  }

  /// Whether a run in `environment` has a value for `name`: its own or the shared one.
  public static func isSet(_ name: String, environment: String?) -> Bool {
    isSet(name) || environment.map { isInKeychain(name, in: service(for: $0)) } ?? false
  }

  /// The environment variable `name`, otherwise the Keychain item of `environment`, otherwise
  /// the shared Keychain item.
  public static func value(_ name: String, environment: String?) -> String? {
    if let environment, let value = value(name, in: service(for: environment)) { return value }
    return value(name)
  }

  public static func isInEnvironment(_ name: String) -> Bool {
    !(ProcessInfo.processInfo.environment[name] ?? "").isEmpty
  }

  public static func isInKeychain(_ name: String, in service: String = tests) -> Bool {
    SecItemCopyMatching(query(name, service) as CFDictionary, nil) == errSecSuccess
  }

  /// The environment variable `name`, otherwise the Keychain item.
  public static func value(_ name: String, in service: String = tests) -> String? {
    if let value = ProcessInfo.processInfo.environment[name], !value.isEmpty { return value }
    return keychainValue(name, in: service)
  }

  /// The Keychain item only, for what an environment variable must never stand in for: keys.
  public static func keychainValue(_ name: String, in service: String = tests) -> String? {
    var item = query(name, service)
    item[kSecReturnData] = true
    item[kSecMatchLimit] = kSecMatchLimitOne
    var data: CFTypeRef?
    guard SecItemCopyMatching(item as CFDictionary, &data) == errSecSuccess, let data = data as? Data else { return nil }
    return String(data: data, encoding: .utf8)
  }

  public static func save(_ value: String, for name: String, in service: String = tests) throws {
    let service = Profile.current.keychainService(service)
    let data = Data(value.utf8)
    var status = SecItemUpdate(item(name, service) as CFDictionary, [kSecValueData: data] as CFDictionary)
    if status == errSecItemNotFound {
      var entry = item(name, service)
      entry[kSecValueData] = data
      entry[kSecAttrLabel] = "\(service): \(name)"
      status = SecItemAdd(entry as CFDictionary, nil)
    }
    try check(status)
  }

  public static func remove(_ name: String, in service: String = tests) throws {
    let status = SecItemDelete(query(name, service) as CFDictionary)
    try check(status == errSecItemNotFound ? errSecSuccess : status)
  }

  private static func check(_ status: OSStatus) throws {
    guard status == errSecSuccess else {
      let message = SecCopyErrorMessageString(status, nil) as String? ?? "\(status)"
      throw NSError(domain: NSOSStatusErrorDomain, code: Int(status), userInfo: [NSLocalizedDescriptionKey: message])
    }
  }

  /// The item of `name` in `service`, in this process's profile.
  private static func query(_ name: String, _ service: String) -> [CFString: Any] {
    item(name, Profile.current.keychainService(service))
  }

  private static func item(_ name: String, _ service: String) -> [CFString: Any] {
    [kSecClass: kSecClassGenericPassword, kSecAttrService: service, kSecAttrAccount: name]
  }
}

/// Secrets for a run, from `SecretStore`.
public final class StoredSecrets: SecretSource {
  public init() {}

  public func secret(name: String, environment: String?) -> String? {
    SecretStore.value(name, environment: environment)
  }
}
