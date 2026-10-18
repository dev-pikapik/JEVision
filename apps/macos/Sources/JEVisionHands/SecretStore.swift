import Foundation
import JEVisionCore
import Security

/// Secret values live in the environment or in the Keychain, never in a test file.
/// Only a run reads a value back, to type it; nothing shows one.
public enum SecretStore {
  static let service = "JEVision"

  public static func isSet(_ name: String) -> Bool {
    if let value = ProcessInfo.processInfo.environment[name], !value.isEmpty { return true }
    return SecItemCopyMatching(query(name) as CFDictionary, nil) == errSecSuccess
  }

  /// The environment variable `name`, otherwise the Keychain item.
  public static func value(_ name: String) -> String? {
    if let value = ProcessInfo.processInfo.environment[name], !value.isEmpty { return value }
    var item = query(name)
    item[kSecReturnData] = true
    item[kSecMatchLimit] = kSecMatchLimitOne
    var data: CFTypeRef?
    guard SecItemCopyMatching(item as CFDictionary, &data) == errSecSuccess, let data = data as? Data else { return nil }
    return String(data: data, encoding: .utf8)
  }

  public static func save(_ value: String, for name: String) throws {
    let data = Data(value.utf8)
    var status = SecItemUpdate(query(name) as CFDictionary, [kSecValueData: data] as CFDictionary)
    if status == errSecItemNotFound {
      var item = query(name)
      item[kSecValueData] = data
      item[kSecAttrLabel] = "JEVision: \(name)"
      status = SecItemAdd(item as CFDictionary, nil)
    }
    guard status == errSecSuccess else {
      let message = SecCopyErrorMessageString(status, nil) as String? ?? "\(status)"
      throw NSError(domain: NSOSStatusErrorDomain, code: Int(status), userInfo: [NSLocalizedDescriptionKey: message])
    }
  }

  private static func query(_ name: String) -> [CFString: Any] {
    [kSecClass: kSecClassGenericPassword, kSecAttrService: service, kSecAttrAccount: name]
  }
}

/// Secrets for a run, from `SecretStore`.
public final class StoredSecrets: SecretSource {
  public init() {}

  public func secret(name: String) -> String? {
    SecretStore.value(name)
  }
}
