import Foundation
import LocalAuthentication
import Security
import Synchronization

/// This iPhone's identity: a P-256 key made in the Secure Enclave. The private half never
/// leaves the chip; Macs know the iPhone only by the public half. Using the key needs the
/// owner (Face ID or passcode), which the app lock already asked for: its authenticated
/// context is handed over in `authenticate(with:)` so signing doesn't ask again.
///
/// The Simulator has no Secure Enclave, so there the key is an ordinary P-256 key in the
/// Simulator's keychain.
final class DeviceKey: Sendable {
  enum KeyError: Error {
    case unavailable(OSStatus)
    case signingFailed(String)
  }

  private static let tag = Data("io.github.dev-pikapik.jevision.remote.device-key".utf8)
  private let context = Mutex<AuthenticatedContext?>(nil)
  /// Held while looking the key up, so two first sign-ins can't each make a key.
  private let lookup = Mutex(())
  private let cachedPublicKey = Mutex<Data?>(nil)

  /// The context the app lock authenticated, or `nil` once the app locks again.
  func authenticate(with context: LAContext?) {
    let box = context.map(AuthenticatedContext.init)
    self.context.withLock { $0 = box }
  }

  /// The public key in SEC1 uncompressed form: 65 bytes starting with `0x04`. Read once,
  /// making the key on first use, then kept in memory.
  func loadPublicKey() throws -> Data {
    if let data = cachedPublicKey.withLock({ $0 }) { return data }
    let key = try privateKey()
    guard let publicKey = SecKeyCopyPublicKey(key) else { throw KeyError.unavailable(errSecInternalError) }
    var error: Unmanaged<CFError>?
    guard let data = SecKeyCopyExternalRepresentation(publicKey, &error) as Data? else {
      throw KeyError.signingFailed(Self.describe(error))
    }
    cachedPublicKey.withLock { $0 = data }
    return data
  }

  /// An ECDSA P-256 signature over the SHA-256 digest of `message`, DER-encoded.
  func sign(_ message: Data) throws -> Data {
    let key = try privateKey()
    var error: Unmanaged<CFError>?
    guard let signature = SecKeyCreateSignature(key, .ecdsaSignatureMessageX962SHA256, message as CFData, &error)
    else { throw KeyError.signingFailed(Self.describe(error)) }
    return signature as Data
  }

  /// The saved key, made on first use.
  private func privateKey() throws -> SecKey {
    try lookup.withLock { _ throws(KeyError) in try findOrMakeKey() }
  }

  private func findOrMakeKey() throws(KeyError) -> SecKey {
    var query: [CFString: Any] = [
      kSecClass: kSecClassKey,
      kSecAttrApplicationTag: Self.tag,
      kSecAttrKeyType: kSecAttrKeyTypeECSECPrimeRandom,
      kSecReturnRef: true,
    ]
    if let box = context.withLock({ $0 }) {
      query[kSecUseAuthenticationContext] = box.context
    }
    var item: CFTypeRef?
    let status = SecItemCopyMatching(query as CFDictionary, &item)
    if status == errSecSuccess, let item {
      return item as! SecKey
    }
    guard status == errSecItemNotFound else { throw KeyError.unavailable(status) }
    return try makeKey()
  }

  private func makeKey() throws(KeyError) -> SecKey {
    var keyAttributes: [CFString: Any] = [
      kSecAttrIsPermanent: true,
      kSecAttrApplicationTag: Self.tag,
    ]
    var attributes: [CFString: Any] = [
      kSecAttrKeyType: kSecAttrKeyTypeECSECPrimeRandom,
      kSecAttrKeySizeInBits: 256,
    ]
    #if targetEnvironment(simulator)
      keyAttributes[kSecAttrAccessible] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
    #else
      var error: Unmanaged<CFError>?
      guard
        let access = SecAccessControlCreateWithFlags(
          nil, kSecAttrAccessibleWhenUnlockedThisDeviceOnly, [.privateKeyUsage, .userPresence], &error)
      else { throw KeyError.signingFailed(Self.describe(error)) }
      keyAttributes[kSecAttrAccessControl] = access
      attributes[kSecAttrTokenID] = kSecAttrTokenIDSecureEnclave
    #endif
    attributes[kSecPrivateKeyAttrs] = keyAttributes
    var createError: Unmanaged<CFError>?
    guard let key = SecKeyCreateRandomKey(attributes as CFDictionary, &createError) else {
      throw KeyError.signingFailed(Self.describe(createError))
    }
    return key
  }

  private static func describe(_ error: Unmanaged<CFError>?) -> String {
    error.map { $0.takeRetainedValue().localizedDescription } ?? "unknown error"
  }
}

/// An evaluated context, handed from the app lock to the threads the core signs on. The
/// Keychain may use one `LAContext` from any thread; it is never mutated after the handover.
private final class AuthenticatedContext: @unchecked Sendable {
  let context: LAContext
  init(_ context: LAContext) { self.context = context }
}
