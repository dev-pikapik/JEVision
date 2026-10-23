import Foundation
import JEVisionCore
import Sparkle
import Testing

@testable import JEVision

@MainActor
struct UpdatesTests {
  private static let stableFeed = "https://example.com/appcast.xml"
  private static let betaFeed = "https://example.com/appcast-beta.xml"

  private func bundle(_ info: [String: String]) throws -> Bundle {
    let url = FileManager.default.temporaryDirectory.appending(path: "updates-\(UUID().uuidString).bundle")
    try FileManager.default.createDirectory(at: url.appending(path: "Contents"), withIntermediateDirectories: true)
    var plist = info
    plist["CFBundleIdentifier"] = "io.github.dev-pikapik.jevision.tests.\(UUID().uuidString)"
    try PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0)
      .write(to: url.appending(path: "Contents/Info.plist"))
    return try #require(Bundle(url: url))
  }

  /// A copy without an update key, so Sparkle isn't started, but with both feeds.
  private func updates(defaults: UserDefaults, environment: [String: String] = [:]) throws -> Updates {
    let bundle = try bundle(["SUFeedURL": Self.stableFeed, "JEVisionBetaFeedURL": Self.betaFeed])
    return Updates(bundle: bundle, defaults: defaults, environment: environment)
  }

  private func scratchDefaults() throws -> UserDefaults {
    try #require(UserDefaults(suiteName: "io.github.dev-pikapik.jevision.tests.\(UUID().uuidString)"))
  }

  @Test func staysOffWithoutAPublicKey() {
    let updates = Updates(bundle: .main)
    #expect(!Updates.hasPublicKey(.main))
    #expect(!updates.isAvailable)
    #expect(!updates.canCheck)
    updates.checksAutomatically = true
    #expect(!updates.checksAutomatically)
  }

  @Test func releasesOnlyUntilBetasAreTurnedOn() throws {
    let defaults = try scratchDefaults()
    let updates = try updates(defaults: defaults)
    #expect(!updates.includesBetas)
    #expect(updates.feedURL == Self.stableFeed)
    #expect(updates.channels.isEmpty)

    updates.includesBetas = true
    #expect(updates.feedURL == Self.betaFeed)
    #expect(updates.channels == [Updates.betaChannel])
    // The choice outlives the launch.
    #expect(defaults.bool(forKey: Updates.betasKey))
    #expect(try self.updates(defaults: defaults).feedURL == Self.betaFeed)
  }

  @Test func theFeedFromTheEnvironmentWinsOnBothChannels() throws {
    let local = "http://localhost:8765/appcast.xml"
    let updates = try updates(defaults: try scratchDefaults(), environment: [Updates.feedOverrideKey: local])
    #expect(updates.feedURL == local)
    updates.includesBetas = true
    #expect(updates.feedURL == local)
    #expect(updates.channels == [Updates.betaChannel])
  }

  @Test(arguments: [
    ("http://localhost:8765/appcast.xml", true), ("https://example.com/feed/appcast-beta.xml", true),
    ("localhost:8765/appcast.xml", false), ("file:///tmp/appcast.xml", false), ("ftp://example.com/appcast.xml", false),
    ("not a feed", false), ("", false),
  ])
  func theEnvironmentGivesOnlyAWebAddress(value: String, taken: Bool) {
    #expect(Updates.feedOverride(in: [Updates.feedOverrideKey: value]) == (taken ? value : nil))
    #expect(Updates.feedOverride(in: [:]) == nil)
  }

  @Test func aBetaIsOlderThanItsReleaseByTheBuildNumber() throws {
    // Sparkle compares CFBundleVersion, the build number, so 1.0.0-beta.1 (2) comes before 1.0.0 (3),
    // and an update with a build number no higher than the installed one is never offered.
    let compare = SUStandardVersionComparator.default
    #expect(compare.compareVersion("2", toVersion: "3") == .orderedAscending)
    #expect(compare.compareVersion("3", toVersion: "3") == .orderedSame)
    #expect(compare.compareVersion("266", toVersion: "1024") == .orderedAscending)
    let beta = AppVersion(bundle: try bundle(["CFBundleShortVersionString": "1.0.0-beta.1", "CFBundleVersion": "2"]))
    #expect(beta.version == "1.0.0-beta.1")
    #expect(beta.build == "2")
  }

  @Test func theInfoPlistChecksDailyAndKnowsItsDataFormat() throws {
    let url = URL(filePath: #filePath)
      .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
      .appending(path: "Resources/Info.plist")
    let info = try #require(
      PropertyListSerialization.propertyList(from: Data(contentsOf: url), format: nil) as? [String: Any])
    #expect(info["SUEnableAutomaticChecks"] as? Bool == true)
    #expect(info["SUScheduledCheckInterval"] as? Int == 86400)
    #expect((info["SUFeedURL"] as? String)?.hasSuffix("/releases/latest/download/appcast.xml") == true)
    #expect((info["JEVisionBetaFeedURL"] as? String)?.hasSuffix("/releases/download/appcast-beta/appcast-beta.xml") == true)
    // A core that reads a newer test format needs a higher JEVisionDataFormat, or no copy is made
    // before the update that brings it.
    let format = try #require(info[UpdateDataGuard.infoKey] as? Int)
    #expect(format >= Int(formatVersion()))
  }

  // The shape make-appcast.sh writes: a release for everyone, a beta on the beta channel, the notes
  // in English and in Russian and the data format of each.
  private static let feed = """
    <?xml version="1.0" encoding="utf-8"?>
    <rss xmlns:jevision="https://github.com/dev-pikapik/JEVision" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle" version="2.0">
      <channel>
        <title>JEVision</title>
        <item>
          <title>1.0.0</title>
          <sparkle:version>3</sparkle:version>
          <sparkle:shortVersionString>1.0.0</sparkle:shortVersionString>
          <description sparkle:format="markdown"><![CDATA[The first release of JEVision.]]></description>
          <description xml:lang="ru" sparkle:format="markdown"><![CDATA[Первый выпуск JEVision.]]></description>
          <jevision:dataFormat>2</jevision:dataFormat>
          <enclosure url="https://example.com/JEVision-1.0.0.dmg" length="1" type="application/octet-stream" sparkle:edSignature="x"/>
        </item>
        <item>
          <title>1.0.0-beta.1</title>
          <sparkle:version>2</sparkle:version>
          <sparkle:shortVersionString>1.0.0-beta.1</sparkle:shortVersionString>
          <sparkle:channel>beta</sparkle:channel>
          <description sparkle:format="markdown"><![CDATA[Beta 1.]]></description>
          <description xml:lang="ru" sparkle:format="markdown"><![CDATA[Бета 1.]]></description>
          <jevision:dataFormat>1</jevision:dataFormat>
          <enclosure url="https://example.com/JEVision-1.0.0-beta.1.dmg" length="1" type="application/octet-stream" sparkle:edSignature="x"/>
        </item>
      </channel>
    </rss>
    """

  @Test func theFeedCarriesWhatTheAppReads() throws {
    let items = try XMLDocument(xmlString: Self.feed).nodes(forXPath: "//item").compactMap { $0 as? XMLElement }
    #expect(items.count == 2)
    for item in items {
      let notes = item.elements(forName: "description")
      #expect(Set(notes.map { $0.attribute(forName: "xml:lang")?.stringValue ?? "en" }) == ["en", "ru"])
      #expect(notes.allSatisfy { $0.attribute(forName: "sparkle:format")?.stringValue == "markdown" })
    }
    let release = items[0], beta = items[1]
    #expect(release.elements(forName: "sparkle:channel").isEmpty)
    #expect(beta.elements(forName: "sparkle:channel").first?.stringValue == Updates.betaChannel)
    let format = release.elements(forName: UpdateDataGuard.feedKey).first?.stringValue
    #expect(UpdateDataGuard.needsBackup(current: 1, dataFormat: format))
    #expect(!UpdateDataGuard.needsBackup(current: 1, dataFormat: beta.elements(forName: UpdateDataGuard.feedKey).first?.stringValue))
  }
}

struct UpdateDataGuardTests {
  private let root = FileManager.default.temporaryDirectory.appending(path: "update-guard-\(UUID().uuidString)")
  private let day = Calendar.current.date(from: DateComponents(year: 2026, month: 10, day: 21, hour: 12))!

  private func write(_ text: String, _ path: String) throws {
    let url = root.appending(path: path)
    try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
    try Data(text.utf8).write(to: url)
  }

  private func exists(_ url: URL, _ path: String) -> Bool {
    FileManager.default.fileExists(atPath: url.appending(path: path).path)
  }

  private func guardian(settings: [String: Any]? = ["updatesIncludeBetas": true]) -> UpdateDataGuard {
    UpdateDataGuard(
      current: 1, folder: root.appending(path: "JEVision"), settings: settings, project: root.appending(path: "Shop"),
      now: day)
  }

  @Test(arguments: [
    ("2", true), (" 2\n", true), ("10", true), ("1", false), ("0", false), ("two", false), ("", false),
  ])
  func onlyANewerFormatNeedsACopy(format: String, needed: Bool) {
    #expect(UpdateDataGuard.needsBackup(current: 1, dataFormat: format) == needed)
    #expect(!UpdateDataGuard.needsBackup(current: 1, dataFormat: nil))
  }

  @Test func noCopyForTheSameFormat() throws {
    #expect(try guardian().prepare(version: "1.0.1", dataFormat: "1") == nil)
    #expect(try guardian().prepare(version: "1.0.1", dataFormat: nil) == nil)
    #expect(!FileManager.default.fileExists(atPath: guardian().backups.path))
  }

  @Test func copiesTheDataSettingsAndProjectButNotItsHistory() throws {
    try write("log", "JEVision/Logs/app.log")
    try write("old", "JEVision/Backups/before-0.9.0-2026-01-01/x")
    try write("{}", "Shop/Login.json")
    try write("ref: refs/heads/main", "Shop/.git/HEAD")
    try write("video", "Shop/.jevision/runs/1/video.mov")

    let copy = try #require(try guardian().prepare(version: "1.1.0", dataFormat: "2"))
    #expect(copy == root.appending(path: "JEVision/Backups/before-1.1.0-2026-10-21", directoryHint: .isDirectory))
    #expect(exists(copy, "Application Support/Logs/app.log"))
    #expect(!exists(copy, "Application Support/Backups"))
    #expect(exists(copy, "Project/Shop/Login.json"))
    #expect(!exists(copy, "Project/Shop/.git"))
    #expect(!exists(copy, "Project/Shop/.jevision"))
    let settings = try PropertyListSerialization.propertyList(
      from: Data(contentsOf: copy.appending(path: "Settings.plist")), format: nil) as? [String: Any]
    #expect(settings?["updatesIncludeBetas"] as? Bool == true)

    // The same update on the same day keeps the first copy, and earlier copies stay.
    try write("changed", "Shop/Login.json")
    #expect(try guardian().prepare(version: "1.1.0", dataFormat: "2") == copy)
    #expect(try String(contentsOf: copy.appending(path: "Project/Shop/Login.json"), encoding: .utf8) == "{}")
    let names = try FileManager.default.contentsOfDirectory(atPath: guardian().backups.path).sorted()
    #expect(names == ["before-0.9.0-2026-01-01", "before-1.1.0-2026-10-21"])
  }

  @Test func aFailedCopyStopsTheUpdateAndLeavesNothingHalfDone() throws {
    try write("log", "JEVision/Logs/app.log")
    let failing = guardian(settings: ["unsaveable": NSNull()])
    let error = try #require(throws: UpdateDataGuard.BackupFailed.self) {
      try failing.prepare(version: "1.1.0", dataFormat: "2")
    }
    #expect(error.version == "1.1.0")
    #expect(error.localizedDescription.contains("1.1.0"))
    #expect(try FileManager.default.contentsOfDirectory(atPath: failing.backups.path).isEmpty)
  }
}
