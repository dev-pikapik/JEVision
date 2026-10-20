import Foundation
import Observation
import Synchronization

/// The app's own language, applied at once: texts come from the folder of that language in the
/// app, dates and numbers from a locale of that language in the Mac's region. The choice is also
/// stored the way macOS expects (`AppleLanguages`), so what macOS draws itself, like the menu bar,
/// follows at the next launch. A change of the Mac's language or region is picked up while running.
final class AppLanguage: Observable, Sendable {
  static let shared = AppLanguage()

  /// Empty means "follow the system".
  static let choices = ["", "en", "ru"]

  private static let key = "AppleLanguages"

  private struct State {
    var saved = ""
    var language = "en"
    var locale = Locale.current
    var bundle: Bundle?
  }

  private let registrar = ObservationRegistrar()
  private let state = Mutex(State())
  /// Not marked Sendable by the SDK, though it is safe to use from any thread.
  nonisolated(unsafe) private let defaults: UserDefaults
  private let domain: String
  private let system: @Sendable () -> [String]
  private let main: Bundle

  /// The language of the choice or, for "System", of the Mac's list that the app has texts for.
  /// `available` are the languages the app has texts for.
  static func resolve(saved: String, system: [String], available: [String]) -> String {
    if !saved.isEmpty { return saved }
    return Bundle.preferredLocalizations(from: available, forPreferences: system).first ?? "en"
  }

  /// `language` as spoken in the region and with the calendar, hours and units the Mac is set to.
  static func locale(language: String, in system: Locale) -> Locale {
    var parts = Locale.Components(locale: system)
    var spoken = Locale.Language.Components(identifier: language)
    spoken.region = system.region ?? spoken.region
    parts.languageComponents = spoken
    return Locale(components: parts)
  }

  /// `domain` is where `defaults` keeps the app's settings; `system` lists the Mac's languages.
  init(
    main: Bundle = .main,
    defaults: UserDefaults = .standard,
    domain: String = Bundle.main.bundleIdentifier ?? "",
    system: @escaping @Sendable () -> [String] = AppLanguage.systemLanguages
  ) {
    self.main = main
    self.defaults = defaults
    self.domain = domain
    self.system = system
    refresh()
  }

  /// Follows changes of the Mac's language and region. Called once, at launch.
  func start() {
    NotificationCenter.default.addObserver(forName: NSLocale.currentLocaleDidChangeNotification, object: nil, queue: .main) {
      [weak self] _ in self?.refresh()
    }
  }

  /// The choice: a language code, or empty for "System".
  var saved: String {
    registrar.access(self, keyPath: \.locale)
    return state.withLock { $0.saved }
  }

  /// The language texts are shown in.
  var language: String {
    registrar.access(self, keyPath: \.locale)
    return state.withLock { $0.language }
  }

  var locale: Locale {
    registrar.access(self, keyPath: \.locale)
    return state.withLock { $0.locale }
  }

  /// The folder of the language in the app; `nil` when the app has none, as under `swift test`.
  var bundle: Bundle? {
    registrar.access(self, keyPath: \.locale)
    return state.withLock { $0.bundle }
  }

  /// Applies a choice at once and keeps it for the next launch.
  func choose(_ code: String) {
    if code.isEmpty {
      defaults.removeObject(forKey: Self.key)
    } else {
      defaults.set([code], forKey: Self.key)
    }
    refresh()
  }

  private func refresh() {
    let saved = (defaults.persistentDomain(forName: domain)?[Self.key] as? [String])?.first ?? ""
    let available = main.localizations.filter { $0 != "Base" }
    let language = Self.resolve(saved: saved, system: system(), available: available.isEmpty ? ["en"] : available)
    let folder = main.path(forResource: language, ofType: "lproj").flatMap { Bundle(path: $0) }
    registrar.withMutation(of: self, keyPath: \.locale) {
      state.withLock {
        $0.saved = saved
        $0.language = language
        $0.locale = Self.locale(language: language, in: .autoupdatingCurrent)
        $0.bundle = folder
      }
    }
  }

  /// The Mac's languages in order of preference, as set now rather than when the app opened.
  static func systemLanguages() -> [String] {
    let global = CFPreferencesCopyValue(key as CFString, kCFPreferencesAnyApplication, kCFPreferencesCurrentUser, kCFPreferencesAnyHost)
    return (global as? [String]) ?? Locale.preferredLanguages
  }

  /// Language names are written in their own language, so they never get translated.
  static func name(_ code: String) -> String {
    let name = Locale(identifier: code).localizedString(forLanguageCode: code) ?? code
    return name.prefix(1).uppercased() + name.dropFirst()
  }
}
