import Foundation
import Security

/// Secret values live in the environment or in the Keychain, never in a test file.
/// The app only checks whether a secret is set and stores new values; it never reads one back.
enum SecretStore {
  static let service = "JEVision"

  static func isSet(_ name: String) -> Bool {
    if let value = ProcessInfo.processInfo.environment[name], !value.isEmpty { return true }
    return SecItemCopyMatching(query(name) as CFDictionary, nil) == errSecSuccess
  }

  static func save(_ value: String, for name: String) throws {
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
