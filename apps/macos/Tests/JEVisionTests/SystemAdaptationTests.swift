import Carbon.HIToolbox
import Foundation
import Synchronization
import JEVisionCore
import Testing

@testable import JEVision

/// A folder shaped like an app with texts in Russian and English.
private func appWithTexts() throws -> Bundle {
  let root = FileManager.default.temporaryDirectory.appending(path: "texts-\(UUID().uuidString)")
  for (language, text) in [("en", "Hello"), ("ru", "Привет")] {
    let folder = root.appending(path: "\(language).lproj")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    try "\"Hello\" = \"\(text)\";\n".write(to: folder.appending(path: "Localizable.strings"), atomically: true, encoding: .utf8)
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
    let language = AppLanguage(main: try appWithTexts(), defaults: suite, domain: name, system: { ["en-US"] })
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

  @Test func theSystemLanguageIsReadAgainWhenTheMacChangesIt() throws {
    let (suite, name) = try defaults()
    defer { suite.removePersistentDomain(forName: name) }
    let languages = SystemLanguages()
    let language = AppLanguage(main: try appWithTexts(), defaults: suite, domain: name, system: { languages.list.withLock { $0 } })
    language.start()
    #expect(language.language == "en")
    languages.list.withLock { $0 = ["ru-RU"] }
    NotificationCenter.default.post(name: NSLocale.currentLocaleDidChangeNotification, object: nil)
    RunLoop.main.run(until: Date().addingTimeInterval(0.2))
    #expect(language.language == "ru")
  }

  @Test func withoutTextsOfItsOwnTheAppKeepsTheMainBundle() throws {
    let (suite, name) = try defaults()
    defer { suite.removePersistentDomain(forName: name) }
    let empty = try #require(Bundle(url: FileManager.default.temporaryDirectory))
    let language = AppLanguage(main: empty, defaults: suite, domain: name, system: { ["ru"] })
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
    #expect(KeyCombo.display("cmd+s", layout: try layout("US")) == "⌘S")
    #expect(KeyCombo.display("cmd+s", layout: try layout("Russian")) == "⌘Ы")
    #expect(KeyCombo.display("cmd+shift+s", layout: try layout("Dvorak")) == "⇧⌘O")
    #expect(KeyCombo.display("cmd+.", layout: try layout("Russian")) == "⌘Ю")
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
