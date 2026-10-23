import Foundation
import Security

/// Whether an app runs in the App Sandbox. macOS gives such an app a home folder of its own, in
/// its container, whatever home folder it is started with.
enum AppSandbox {
  static let entitlement = "com.apple.security.app-sandbox"

  /// Reads the entitlements the app's code signature carries; an app without a signature can't
  /// be in the sandbox.
  static func contains(_ url: URL) -> Bool {
    var code: SecStaticCode?
    guard SecStaticCodeCreateWithPath(url as CFURL, [], &code) == errSecSuccess, let code else { return false }
    var info: CFDictionary?
    guard SecCodeCopySigningInformation(code, SecCSFlags(rawValue: kSecCSSigningInformation), &info) == errSecSuccess,
      let info = info as? [String: Any],
      let entitlements = info[kSecCodeInfoEntitlementsDict as String] as? [String: Any]
    else { return false }
    return entitlements[entitlement] as? Bool == true
  }
}
