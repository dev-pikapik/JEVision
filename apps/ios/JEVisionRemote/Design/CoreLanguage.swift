import Foundation
import JEVisionMobile

/// The language the core words failures in: the one the phone shows the app in.
enum CoreLanguage {
  /// Hands the first of `preferred` (as the system writes it, `ru`, `en-GB`) to the core; a
  /// language without a catalog is worded in English.
  static func apply(_ preferred: [String] = Bundle.main.preferredLocalizations) {
    setLanguage(tag: preferred.first ?? "en")
  }
}
