import Foundation

/// The app's own language, stored the way macOS expects it: `AppleLanguages` in the app's defaults.
/// macOS reads it only at launch, so a change takes effect after a restart.
enum AppLanguage {
  /// Empty means "follow the system".
  static let choices = ["", "en", "ru"]

  private static let key = "AppleLanguages"

  static var saved: String {
    let domain = UserDefaults.standard.persistentDomain(forName: Bundle.main.bundleIdentifier ?? "")
    return (domain?[key] as? [String])?.first ?? ""
  }

  static let atLaunch = saved

  static func save(_ code: String) {
    if code.isEmpty {
      UserDefaults.standard.removeObject(forKey: key)
    } else {
      UserDefaults.standard.set([code], forKey: key)
    }
  }

  /// Language names are written in their own language, so they never get translated.
  static func name(_ code: String) -> String {
    let name = Locale(identifier: code).localizedString(forLanguageCode: code) ?? code
    return name.prefix(1).uppercased() + name.dropFirst()
  }
}
