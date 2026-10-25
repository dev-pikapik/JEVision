import AppKit
import JEVisionHands
import SwiftUI
import Testing

@testable import JEVision
@testable import JEVisionCLI

private struct PreferencesFixture {
  let root: URL
  let profile: Profile
  var file: URL { root.appending(path: "Settings.plist") }

  init(_ name: String = "v1") throws {
    root = FileManager.default.temporaryDirectory.appending(path: "jevision-preferences-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    let source = try #require(Bundle.module.url(forResource: name, withExtension: "plist", subdirectory: "Fixtures/Preferences"))
    try FileManager.default.copyItem(at: source, to: root.appending(path: "Settings.plist"))
    profile = Profile(folder: root)
    profile.defaults.synchronize()
  }

  func discard() {
    profile.defaults.removePersistentDomain(forName: profile.defaultsDomain)
    profile.defaults.synchronize()
    try? FileManager.default.removeItem(at: root)
  }

  func replaceHeader(_ value: Any) throws -> Data {
    let data = try PropertyListSerialization.data(fromPropertyList: [AppPreferences.formatKey: value, "future": ["keep": true]],
      format: .xml, options: 0)
    try data.write(to: file, options: .atomic)
    return data
  }
}

struct AppPreferencesTests {
  @Test func legacyMigrationBacksUpBeforeStampingAndPreservesUnfamiliarValues() throws {
    let fixture = try PreferencesFixture("legacy")
    defer { fixture.discard() }
    let bytes = try Data(contentsOf: fixture.file)
    let defaults = fixture.profile.defaults
    let original = try #require(defaults.persistentDomain(forName: fixture.profile.defaultsDomain))
    try AppPreferences.check(defaults)
    #expect(try Data(contentsOf: fixture.file) == bytes, "read-only checks never migrate")
    #expect(defaults.object(forKey: AppPreferences.formatKey) == nil)

    let backup = try #require(try AppPreferences.prepareForGUI(fixture.profile))
    #expect(try Data(contentsOf: backup.appending(path: "Settings.original.plist")) == bytes)
    let saved = try PropertyListSerialization.propertyList(from: Data(contentsOf: backup.appending(path: "Settings.plist")), format: nil)
    #expect((saved as? NSDictionary) == original as NSDictionary)
    for (file, mode) in [(backup, 0o700), (backup.deletingLastPathComponent(), 0o700), (backup.appending(path: "Settings.plist"), 0o600)] {
      #expect((try FileManager.default.attributesOfItem(atPath: file.path)[.posixPermissions] as? Int) == mode)
    }
    #expect(defaults.integer(forKey: AppPreferences.formatKey) == 1)
    #expect(AppPreferences.set("light", forKey: "appearance", in: defaults))
    for key in ["future.data", "future.date", "future.nested"] {
      #expect(NSDictionary(dictionary: [key: defaults.object(forKey: key) as Any]) == NSDictionary(dictionary: [key: original[key] as Any]))
    }
    #expect(try AppPreferences.prepareForGUI(fixture.profile) == nil, "migration runs only once")
  }

  @Test func aReplacedLegacyFileGetsItsOwnBackupDespiteACachedCurrentMarker() throws {
    let fixture = try PreferencesFixture()
    defer { fixture.discard() }
    #expect(fixture.profile.defaults.integer(forKey: AppPreferences.formatKey) == 1)
    let source = try #require(Bundle.module.url(forResource: "legacy", withExtension: "plist", subdirectory: "Fixtures/Preferences"))
    var restored = try #require(PropertyListSerialization.propertyList(from: Data(contentsOf: source), format: nil) as? [String: Any])
    restored["restored.only"] = ["kept": true]
    restored["appearance"] = "restored-appearance"
    let replaced = try PropertyListSerialization.data(fromPropertyList: restored, format: .xml, options: 0)
    try replaced.write(to: fixture.file, options: .atomic)
    let backup = try #require(try AppPreferences.prepareForGUI(fixture.profile))
    #expect(try Data(contentsOf: backup.appending(path: "Settings.original.plist")) == replaced)
    let stored = try PropertyListSerialization.propertyList(from: Data(contentsOf: fixture.file), format: nil) as? [String: Any]
    #expect(stored?[AppPreferences.formatKey] as? Int == 1)
    #expect(stored?["appearance"] as? String == "restored-appearance")
    #expect(stored?["restored.only"] as? [String: Bool] == ["kept": true])
  }

  @Test func aFailedBackupNeverAddsTheMarker() throws {
    let fixture = try PreferencesFixture("legacy")
    defer { fixture.discard() }
    let before = try Data(contentsOf: fixture.file)
    let blocked = fixture.profile.supportFolder.appending(path: "Preferences Backups")
    try FileManager.default.createDirectory(at: blocked.deletingLastPathComponent(), withIntermediateDirectories: true)
    try Data("occupied".utf8).write(to: blocked)
    #expect(throws: AppPreferences.Failure.self) { try AppPreferences.prepareForGUI(fixture.profile) }
    #expect(try Data(contentsOf: fixture.file) == before)
    #expect(fixture.profile.defaults.object(forKey: AppPreferences.formatKey) == nil)
  }

  @Test func diskRequirementsCannotBeHiddenByCachedOrArgumentDefaults() throws {
    let fixture = try PreferencesFixture()
    defer { fixture.discard() }
    let defaults = fixture.profile.defaults
    #expect(defaults.integer(forKey: AppPreferences.formatKey) == 1)
    defaults.setVolatileDomain([AppPreferences.formatKey: 1], forName: UserDefaults.argumentDomain)
    defaults.register(defaults: [AppPreferences.formatKey: 1])
    let bytes = try fixture.replaceHeader(99)
    do {
      try AppPreferences.check(defaults)
      Issue.record("A cached supported version must not hide the future file")
    } catch let error as AppPreferences.Failure { #expect(error.status == 5) }
    #expect(!AppPreferences.set("changed", forKey: "appearance", in: defaults))
    #expect(!AppPreferences.removeObject(forKey: "appearance", in: defaults))
    #expect(try Data(contentsOf: fixture.file) == bytes)
  }

  @Test func onlyPositiveIntegerHeadersAreAcceptedAndCorruptFilesAreNotEmptyDomains() throws {
    for value: Any in [false, "1", 0, -1, 1.0, 1.5, [1]] {
      let fixture = try PreferencesFixture()
      defer { fixture.discard() }
      let bytes = try fixture.replaceHeader(value)
      do {
        try AppPreferences.check(fixture.profile.defaults)
        Issue.record("Invalid preferences header was accepted: \(value)")
      } catch let error as AppPreferences.Failure { #expect(error.status == 3) }
      #expect(try Data(contentsOf: fixture.file) == bytes)
    }
    for bytes in [Data(), Data("not a plist".utf8), try PropertyListSerialization.data(fromPropertyList: ["array"], format: .xml, options: 0)] {
      let fixture = try PreferencesFixture()
      defer { fixture.discard() }
      try bytes.write(to: fixture.file, options: .atomic)
      #expect(throws: AppPreferences.Failure.self) { try AppPreferences.check(fixture.profile.defaults) }
      #expect(!AppPreferences.set(true, forKey: "a", in: fixture.profile.defaults))
      #expect(try Data(contentsOf: fixture.file) == bytes)
    }
  }

  @MainActor @Test(arguments: ["future", "malformed"])
  func incompatibleFilesStopAllGUIInitializersAndResetWithoutChangingBytes(_ name: String) throws {
    let fixture = try PreferencesFixture(name)
    defer { fixture.discard() }
    let bytes = try Data(contentsOf: fixture.file)
    var initialized = false
    #expect(throws: AppPreferences.Failure.self) {
      try Launch.startGUI(fixture.profile, application: { initialized = true })
    }
    #expect(!initialized)
    #expect(throws: AppPreferences.Failure.self) {
      try SettingsReset.reset(fixture.profile.defaults, domain: fixture.profile.defaultsDomain)
    }
    #expect(try Data(contentsOf: fixture.file) == bytes)
  }

  @MainActor @Test func resetKeepsTheSupportedFormatAndVideoRetentionCannotOverflow() throws {
    let fixture = try PreferencesFixture()
    defer { fixture.discard() }
    try SettingsReset.reset(fixture.profile.defaults, domain: fixture.profile.defaultsDomain)
    #expect(fixture.profile.defaults.integer(forKey: AppPreferences.formatKey) == 1)
    let video = VideoSettings(fixture.profile.defaults)
    for invalid in [Int.max, -1, Int(UInt32.max) + 1] {
      fixture.profile.defaults.set(invalid, forKey: VideoSettings.keepDaysKey)
      #expect(video.keepDays == 30)
    }
    fixture.profile.defaults.set(0, forKey: VideoSettings.keepDaysKey)
    #expect(video.keepDays == nil)
  }

  @Test func aLongLivedAssistantRefusesActionsAfterPreferencesBecomeNewer() throws {
    let fixture = try PreferencesFixture()
    defer { fixture.discard() }
    let host = MacAssistantHost(defaults: fixture.profile.defaults, readiness: { "test readiness" })
    #expect(host.readinessProblem() == "test readiness")
    let bytes = try fixture.replaceHeader(2)
    let permissions = host.permissions()
    #expect(!permissions.runTests && !permissions.screenshots && permissions.askFirst)
    #expect(host.readinessProblem() != "test readiness")
    #expect(host.runOptions().localOnly)
    #expect(host.runOptions().video == .off)
    #expect(throws: AppPreferences.Failure.self) { try host.capture(app: "com.example.NeverOpen") }
    #expect(try Data(contentsOf: fixture.file) == bytes)
  }
}

/// A real SwiftUI binding must use the injected profile, and recheck before writing its key.
@MainActor
@Suite(.enabled(if: ProcessInfo.processInfo.environment["JEVISION_SCALE"] == "1"), .serialized)
struct PreferenceBindingTests {
  private struct Probe: View {
    @AppPreference("probe") private var flag = false
    var body: some View { Button("Change", systemImage: "checkmark") { flag.toggle() }.buttonStyle(.borderless) }
  }

  @Test func bindingsCheckTheSameStoreTheyDisplay() async throws {
    let fixture = try PreferencesFixture()
    defer { fixture.discard() }
    let host = NSHostingView(rootView: Probe().appPreferences(fixture.profile.defaults))
    let window = NSWindow(contentRect: NSRect(x: -10000, y: -10000, width: 300, height: 200),
      styleMask: [.titled], backing: .buffered, defer: false)
    window.isReleasedWhenClosed = false
    window.contentView = host
    window.orderBack(nil)
    defer { window.contentView = nil; window.close() }
    host.layoutSubtreeIfNeeded()
    window.displayIfNeeded()
    try await Task.sleep(for: .milliseconds(300))
    func find(_ view: NSView) -> NSButton? {
      if let button = view as? NSButton { return button }
      return view.subviews.lazy.compactMap(find).first
    }
    let button = try #require(find(host))
    button.performClick(nil)
    #expect(fixture.profile.defaults.bool(forKey: "probe"))
    fixture.profile.defaults.synchronize()
    let bytes = try fixture.replaceHeader(2)
    button.performClick(nil)
    #expect(fixture.profile.defaults.bool(forKey: "probe"), "refusal preserves the displayed store")
    #expect(try Data(contentsOf: fixture.file) == bytes)
  }
}
