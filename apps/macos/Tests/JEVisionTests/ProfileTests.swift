import AppKit
import JEVisionHands
import Testing

@testable import JEVision

struct ProfileTests {
  private func scratch() throws -> URL {
    let made = FileManager.default.temporaryDirectory.appending(path: "jevision-profile-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: made, withIntermediateDirectories: true)
    return made
  }

  private func discard(_ profile: Profile) throws {
    profile.defaults.removePersistentDomain(forName: profile.defaultsDomain)
    try FileManager.default.removeItem(at: try #require(profile.folder))
  }

  @Test func withoutAProfileTheAppKeepsItsDataWhereItAlwaysHas() {
    let app = Profile(environment: [:], current: "/work", bundleIdentifier: AISettings.appDomain)
    #expect(!app.isolated)
    #expect(app.folder == nil)
    #expect(app.supportFolder == URL.applicationSupportDirectory.appending(path: "JEVision", directoryHint: .isDirectory))
    #expect(app.logsFolder == URL.libraryDirectory.appending(path: "Logs/JEVision", directoryHint: .isDirectory))
    #expect(app.defaults === UserDefaults.standard)
    #expect(app.appDefaults === UserDefaults.standard)
    #expect(app.defaultsDomain == AISettings.appDomain)
    #expect(app.appDefaultsDomain == AISettings.appDomain)
    #expect(app.keychainService("JEVision GitHub") == "JEVision GitHub")

    let tool = Profile(environment: [Profile.variable: ""], current: "/work", bundleIdentifier: nil)
    #expect(!tool.isolated)
    #expect(tool.defaults === UserDefaults.standard)
    #expect(tool.appDefaults !== UserDefaults.standard)
    #expect(tool.appDefaultsDomain == AISettings.appDomain)
  }

  @Test func aProfileKeepsEverythingInItsFolder() {
    let profile = Profile(
      environment: [Profile.variable: "self/profile/"], current: "/work", bundleIdentifier: AISettings.appDomain)
    #expect(profile.isolated)
    #expect(profile.folder?.path(percentEncoded: false) == "/work/self/profile/")
    #expect(profile.supportFolder.path(percentEncoded: false) == "/work/self/profile/Support/")
    #expect(profile.logsFolder.path(percentEncoded: false) == "/work/self/profile/Logs/")
    #expect(profile.defaultsDomain == "/work/self/profile/Settings")
    #expect(profile.appDefaultsDomain == profile.defaultsDomain)
    #expect(profile.defaults !== UserDefaults.standard)
    #expect(profile.appDefaults === profile.defaults)
    #expect(profile.keychainService("JEVision GitHub") == "JEVision GitHub (profile /work/self/profile)")
  }

  @Test func settingsOfAProfileStayInIt() throws {
    let key = "profileTests.\(UUID().uuidString)"
    let one = Profile(folder: try scratch())
    let other = Profile(folder: try scratch())
    defer {
      try? discard(one)
      try? discard(other)
    }

    one.defaults.set("kept", forKey: key)
    #expect(Profile(folder: one.folder).defaults.string(forKey: key) == "kept")
    #expect(one.defaults.persistentDomain(forName: one.defaultsDomain)?[key] as? String == "kept")
    #expect(other.defaults.string(forKey: key) == nil)
    #expect(UserDefaults.standard.string(forKey: key) == nil)
    #expect(UserDefaults(suiteName: AISettings.appDomain)?.string(forKey: key) == nil)
  }

  @Test func aProfileLaunchesWithItsOwnLanguageAndNoRestoredWindows() throws {
    let profile = Profile(folder: try scratch())
    defer { try? discard(profile) }

    let fresh = Launch.arguments(profile, current: ["NSDocumentRevisionsDebugMode": "YES"], system: ["de-DE", "en"])
    #expect(fresh["ApplePersistenceIgnoreState"] as? Bool == true)
    #expect(fresh["AppleLanguages"] as? [String] == ["de-DE", "en"])
    #expect(fresh["NSDocumentRevisionsDebugMode"] as? String == "YES")

    profile.defaults.set(["ru"], forKey: "AppleLanguages")
    #expect(Launch.arguments(profile, current: [:], system: ["de-DE"])["AppleLanguages"] as? [String] == ["ru"])
  }

  @MainActor @Test func aProfileSavesNoPlacesOfWindowsInThePersonsSettings() {
    let center = NotificationCenter()
    Launch.forgetWindowPlaces(center)
    let split = NSSplitView()
    split.autosaveName = "main, SidebarNavigationSplitView"
    center.post(name: NSSplitView.willResizeSubviewsNotification, object: split)
    #expect(split.autosaveName == nil)
    let other = NSSplitView()
    other.autosaveName = "other"
    center.post(name: NSSplitView.didResizeSubviewsNotification, object: other)
    #expect(other.autosaveName == "other")
  }
}
