import Foundation
import SwiftUI

// The app's own language reaches every text and format through these, so a change of the language
// shows at once. In this module they stand in for the ones of Foundation with the same names: the
// texts of `String(localized:)` come from the app's language folder, and `formatted` writes dates,
// numbers, lists and durations in the app's locale. Views get the same locale from `appLocale()`.
// A text with a key of its own (`localized:` and `defaultValue:`) can't be stood in for without the
// compiler losing its key, so it passes `bundle: .app, locale: .app` itself; the script
// scripts/check-localization.py fails when one doesn't.

extension Bundle {
  /// The folder of the app's language in the app, for texts that name it themselves.
  static var app: Bundle { AppLanguage.shared.bundle ?? .main }
}

extension Locale {
  static var app: Locale { AppLanguage.shared.locale }
}

extension String {
  init(localized keyAndValue: String.LocalizationValue, table: String? = nil, locale: Locale? = nil, comment: StaticString? = nil) {
    let language = AppLanguage.shared
    self.init(localized: keyAndValue, table: table, bundle: language.bundle, locale: locale ?? language.locale, comment: comment)
  }
}

extension Date {
  func formatted<S: Foundation.FormatStyle>(_ style: S) -> S.FormatOutput where S.FormatInput == Date {
    style.locale(AppLanguage.shared.locale).format(self)
  }

  func formatted(date: Date.FormatStyle.DateStyle, time: Date.FormatStyle.TimeStyle) -> String {
    Date.FormatStyle(date: date, time: time).locale(AppLanguage.shared.locale).format(self)
  }
}

extension BinaryInteger {
  func formatted<S: Foundation.FormatStyle>(_ style: S) -> S.FormatOutput where S.FormatInput == Self {
    style.locale(AppLanguage.shared.locale).format(self)
  }
}

extension BinaryFloatingPoint {
  func formatted<S: Foundation.FormatStyle>(_ style: S) -> S.FormatOutput where S.FormatInput == Self {
    style.locale(AppLanguage.shared.locale).format(self)
  }
}

extension Sequence {
  func formatted<S: Foundation.FormatStyle>(_ style: S) -> S.FormatOutput where S.FormatInput == Self {
    style.locale(AppLanguage.shared.locale).format(self)
  }
}

extension Duration {
  func formatted<S: Foundation.FormatStyle>(_ style: S) -> S.FormatOutput where S.FormatInput == Duration {
    style.locale(AppLanguage.shared.locale).format(self)
  }
}

private struct AppLocale: ViewModifier {
  func body(content: Content) -> some View {
    content.environment(\.locale, AppLanguage.shared.locale)
  }
}

extension View {
  /// Gives the view the app's locale, and redraws it when the language or region changes.
  func appLocale() -> some View {
    modifier(AppLocale())
  }
}
