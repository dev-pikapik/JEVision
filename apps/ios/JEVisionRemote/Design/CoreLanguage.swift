import Foundation
import JEVisionMobile

/// The language the core words failures in: the one the phone shows the app in.
enum CoreLanguage {
  /// Hands the first of `preferred` (as the system writes it, `ru`, `en-GB`) to the core, its
  /// numbers written the way that language is written in the phone's region; a language without a
  /// catalog is worded in English.
  static func apply(_ preferred: [String] = Bundle.main.preferredLocalizations, region: Locale = .current) {
    let language = preferred.first ?? "en"
    setLanguage(tag: language)
    var spoken = Locale.Language.Components(identifier: language)
    spoken.region = region.region ?? spoken.region
    var parts = Locale.Components(locale: region)
    parts.languageComponents = spoken
    let locale = Locale(components: parts)
    setNumberFormat(decimal: locale.decimalSeparator ?? ".", grouping: locale.groupingSeparator ?? "")
  }
}
