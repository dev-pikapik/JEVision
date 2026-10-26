import Foundation
import JEVisionCore

/// The language a run writes month and day names in when a date and time block names none: the
/// one chosen for the app in Settings → General, else the first of the Mac's languages a date
/// block knows. The app and the `jevision` tool read the same choice.
public enum RunLanguage {
  public static var app: String { language(Profile.current.appDefaults) }

  /// The app keeps its choice the way macOS does, as `AppleLanguages`; without one, the Mac's own
  /// list shows through.
  public static func language(_ defaults: any PreferenceSource) -> String {
    let defaults = defaults.preferenceStore
    let preferred = defaults.stringArray(forKey: "AppleLanguages") ?? Locale.preferredLanguages
    return Bundle.preferredLocalizations(from: dateLanguages(), forPreferences: preferred).first ?? "en"
  }
}
