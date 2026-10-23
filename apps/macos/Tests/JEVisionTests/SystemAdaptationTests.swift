import Carbon.HIToolbox
import Foundation
import Synchronization
import JEVisionCore
import Testing

@testable import JEVision

/// A folder shaped like an app with texts in Russian and English.
private func appWithTexts(declaredLanguages: [String]? = nil) throws -> Bundle {
  let root = FileManager.default.temporaryDirectory.appending(path: "texts-\(UUID().uuidString)")
  for (language, text) in [("en", "Hello"), ("ru", "Привет")] {
    let folder = root.appending(path: "\(language).lproj")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    try "\"Hello\" = \"\(text)\";\n".write(to: folder.appending(path: "Localizable.strings"), atomically: true, encoding: .utf8)
  }
  if let declaredLanguages {
    try PropertyListSerialization.data(fromPropertyList: ["CFBundleLocalizations": declaredLanguages], format: .xml, options: 0)
      .write(to: root.appending(path: "Info.plist"))
  }
  return try #require(Bundle(url: root))
}

/// The Mac's languages, changed by a test while the app looks at them.
private final class SystemLanguages: Sendable {
  let list = Mutex(["en-US"])
}

private func defaults() throws -> (UserDefaults, String) {
  let name = "language-\(UUID().uuidString)"
  return (try #require(UserDefaults(suiteName: name)), name)
}

struct AppLanguageTests {
  @Test func aChosenLanguageWinsOverTheSystem() {
    #expect(AppLanguage.resolve(saved: "ru", system: ["en-US"], available: ["en", "ru"]) == "ru")
  }

  @Test func systemFollowsTheFirstLanguageTheAppHasTextsFor() {
    #expect(AppLanguage.resolve(saved: "", system: ["de-DE", "ru-RU", "en-US"], available: ["en", "ru"]) == "ru")
    #expect(AppLanguage.resolve(saved: "", system: ["de-DE", "fr-FR"], available: ["en", "ru"]) == "en")
  }

  @Test func languagesTheAppIsStartedWithStandInForTheMacs() {
    #expect(AppLanguage.launchLanguages(in: ["JEVision", "-AppleLanguages", "(en)"]) == ["en"])
    #expect(AppLanguage.launchLanguages(in: ["JEVision", "-AppleLanguages", "(\"de-DE\", en)", "-x"]) == ["de-DE", "en"])
    #expect(AppLanguage.launchLanguages(in: ["JEVision", "-AppleLanguages", "ru"]) == ["ru"])
    #expect(AppLanguage.launchLanguages(in: ["JEVision", "-AppleLanguages", "()"]) == nil)
    #expect(AppLanguage.launchLanguages(in: ["JEVision", "-AppleLanguages"]) == nil)
    #expect(AppLanguage.launchLanguages(in: ["JEVision"]) == nil)
  }

  @Test func theLocaleSpeaksTheLanguageInTheRegionOfTheMac() {
    let locale = AppLanguage.locale(language: "ru", in: Locale(identifier: "en_US"))
    #expect(locale.language.languageCode == .russian)
    #expect(locale.region == .unitedStates)
    let date = Date(timeIntervalSince1970: 0)
    #expect(Date.FormatStyle().month(.wide).locale(locale).format(date).hasPrefix("янв"))
  }

  @Test func choosingALanguageAppliesAtOnceAndIsKept() throws {
    let (suite, name) = try defaults()
    defer { suite.removePersistentDomain(forName: name) }
    let language = AppLanguage(main: try appWithTexts(), defaults: suite, domain: name, system: { ["en-US"] }, apply: { _, _, _ in })
    #expect(language.saved == "")
    #expect(language.language == "en")
    language.choose("ru")
    #expect(language.saved == "ru")
    #expect(language.language == "ru")
    #expect(language.locale.language.languageCode == .russian)
    #expect(suite.array(forKey: "AppleLanguages") as? [String] == ["ru"])
    #expect(String(localized: "Hello", bundle: language.bundle) == "Привет")
    language.choose("")
    #expect(language.language == "en")
    #expect(String(localized: "Hello", bundle: language.bundle) == "Hello")
    #expect(suite.persistentDomain(forName: name)?["AppleLanguages"] == nil)
  }

  @Test func theLanguageInUseGoesToTheCoreAtLaunchAndAtEveryChange() throws {
    let (suite, name) = try defaults()
    defer { suite.removePersistentDomain(forName: name) }
    let sent = Mutex<[String]>([])
    let language = AppLanguage(
      main: try appWithTexts(), defaults: suite, domain: name, system: { ["en-US"] }, apply: { tag, _, _ in sent.withLock { $0.append(tag) } })
    language.choose("ru")
    language.choose("")
    #expect(sent.withLock { $0 } == ["en", "ru", "en"])
  }

  @Test func numbersGoToTheCoreWithTheSeparatorsOfTheLocaleInUse() throws {
    let (suite, name) = try defaults()
    defer { suite.removePersistentDomain(forName: name) }
    let sent = Mutex<[String]>([])
    let language = AppLanguage(
      main: try appWithTexts(), defaults: suite, domain: name, system: { ["en-US"] },
      apply: { tag, decimal, grouping in sent.withLock { $0 = [tag, decimal, grouping] } })
    language.choose("ru")
    let locale = language.locale
    #expect(locale.language.languageCode == .russian)
    #expect(sent.withLock { $0 } == ["ru", locale.decimalSeparator ?? ".", locale.groupingSeparator ?? ""])
  }

  /// A tag the catalog has no texts for, so that no wording elsewhere changes while it is set.
  @Test func theCoreReceivesTheLanguageThroughTheDefaultHandOver() throws {
    let (suite, name) = try defaults()
    defer { suite.removePersistentDomain(forName: name) }
    let before = currentLanguage()
    defer {
      setLanguage(tag: before)
      setNumberFormat(decimal: "", grouping: "")
    }
    _ = AppLanguage(main: try appWithTexts(), defaults: suite, domain: name, system: { ["en-US"] })
    #expect(currentLanguage() == "en")
    let other = AppLanguage(main: try appWithTexts(), defaults: suite, domain: name, system: { ["en-US"] })
    other.choose("de")
    #expect(currentLanguage() == "de")
  }

  @Test func theChoicesAreTheLanguagesOfTheAppWithEnglishFirst() throws {
    #expect(AppLanguage.languages(in: try appWithTexts()).first == "en")
    let empty = try #require(Bundle(url: FileManager.default.temporaryDirectory))
    #expect(AppLanguage.languages(in: empty) == ["en"])
    #expect(AppLanguage.choices.first == "")
  }

  @Test func languagesDeclaredInMetadataAndResourceFoldersAreOfferedOnce() throws {
    let bundle = try appWithTexts(declaredLanguages: ["en", "ru"])
    #expect(AppLanguage.languages(in: bundle) == ["en", "ru"])
  }

  @Test func theSystemLanguageIsReadAgainWhenTheMacChangesIt() throws {
    let (suite, name) = try defaults()
    defer { suite.removePersistentDomain(forName: name) }
    let languages = SystemLanguages()
    let language = AppLanguage(main: try appWithTexts(), defaults: suite, domain: name, system: { languages.list.withLock { $0 } }, apply: { _, _, _ in })
    language.start()
    #expect(language.language == "en")
    languages.list.withLock { $0 = ["ru-RU"] }
    NotificationCenter.default.post(name: NSLocale.currentLocaleDidChangeNotification, object: nil)
    try eventually("the language is read again") { language.language == "ru" }
    #expect(language.language == "ru")
  }

  @Test func withoutTextsOfItsOwnTheAppKeepsTheMainBundle() throws {
    let (suite, name) = try defaults()
    defer { suite.removePersistentDomain(forName: name) }
    let empty = try #require(Bundle(url: FileManager.default.temporaryDirectory))
    let language = AppLanguage(main: empty, defaults: suite, domain: name, system: { ["ru"] }, apply: { _, _, _ in })
    #expect(language.bundle == nil)
  }
}

@MainActor
struct KeyboardLayoutTests {
  private func layout(_ id: String) throws -> Data {
    let sources = TISCreateInputSourceList(
      [kTISPropertyInputSourceID as String: "com.apple.keylayout.\(id)"] as CFDictionary, true)?.takeRetainedValue()
    let source = try #require((sources as? [TISInputSource])?.first)
    return try #require(KeyboardLayout.data(of: source))
  }

  @Test func aKeyShowsWhatTheLayoutPrintsOnIt() throws {
    #expect(KeyCombo.display("s", layout: try layout("US")) == "S")
    #expect(KeyCombo.display("s", layout: try layout("Russian")) == "Ы")
    #expect(KeyCombo.display("shift+s", layout: try layout("Dvorak")) == "⇧O")
    #expect(KeyCombo.display("ctrl+.", layout: try layout("Russian")) == "⌃Ю")
  }

  @Test func aCommandShortcutShowsTheLatinLetterOnAnyLayout() throws {
    let russian = try layout("Russian"), us = try layout("US")
    #expect(KeyCombo.display("cmd+a", layout: russian, latinLayout: us) == "⌘A")
    #expect(KeyCombo.display("cmd+v", layout: russian, latinLayout: us) == "⌘V")
    #expect(KeyCombo.display("cmd+q", layout: russian, latinLayout: us) == "⌘Q")
    #expect(KeyCombo.display("cmd+.", layout: russian, latinLayout: us) == "⌘.")
    #expect(KeyCombo.display("cmd+shift+s", layout: russian, latinLayout: try layout("Dvorak")) == "⇧⌘O")
    #expect(KeyCombo.display("cmd+s", layout: nil, latinLayout: nil) == "⌘S")
  }

  @Test func theLatinLayoutIsReadWithTheCurrentOne() {
    let keyboard = ActiveKeyboard()
    keyboard.start()
    #expect(keyboard.latinLayout != nil)
  }

  @Test func specialKeysKeepTheirSystemSymbolsOnEveryLayout() throws {
    for id in ["US", "Russian", "Dvorak"] {
      let layout = try layout(id)
      #expect(KeyCombo.display("ctrl+option+return", layout: layout) == "⌃⌥↩")
      #expect(KeyCombo.display("cmd+left", layout: layout) == "⌘←")
      #expect(KeyCombo.display("escape", layout: layout) == "⎋")
      #expect(KeyCombo.display("f5", layout: layout) == "F5")
    }
  }

  @Test func withoutALayoutAKeyShowsItsUSName() {
    #expect(KeyCombo.display("cmd+s", layout: nil) == "⌘S")
  }

  @Test func theLayoutInUseIsRead() throws {
    let keyboard = ActiveKeyboard()
    keyboard.start()
    #expect(keyboard.layout != nil)
  }
}

struct StatusShapeTests {
  @Test func everyStatusHasAShapeOfItsOwn() {
    let dots = RunStatus.allCases.map(\.dot)
    #expect(Set(dots).count == dots.count)
  }

  @Test func aHealthMarkIsDrawnAsTheStatusItStandsFor() {
    #expect(HealthMark.passed.status == .passed)
    #expect(HealthMark.flaky.status == .flaky)
    #expect(HealthMark.failed.status == .failed)
  }
}

struct HelpGuideTests {
  @Test func aGuideOpensFromTheAppWhenItHasTheDocumentation() throws {
    let root = FileManager.default.temporaryDirectory.appending(path: "docs-\(UUID().uuidString)")
    let docs = root.appending(path: "docs")
    try FileManager.default.createDirectory(at: docs, withIntermediateDirectories: true)
    try "<html></html>".write(to: docs.appending(path: "faq.html"), atomically: true, encoding: .utf8)
    let bundle = try #require(Bundle(url: root))
    #expect(Guide.faq.url(in: bundle).isFileURL)
    #expect(Guide.checks.url(in: bundle).host() == "github.com")
  }
}
