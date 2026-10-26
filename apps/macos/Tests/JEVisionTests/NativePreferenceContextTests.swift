import Foundation
import JEVisionHands
import Observation
import Synchronization
import Testing

@testable import JEVision

@MainActor
@Suite(.serialized)
struct NativePreferenceContextTests {
  @Test(arguments: ["enabled", "video.enabled"])
  func anotherProcessInvalidatesTheNativePreferenceWithoutAReadOrSynchronize(_ key: String) async throws {
    let domain = "test.jevision.native-preference-context." + UUID().uuidString
    let native = try #require(UserDefaults(suiteName: domain))
    let folder = FileManager.default.temporaryDirectory.appending(path: domain)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true,
      attributes: [.posixPermissions: 0o700])
    let file = FileManager.default.homeDirectoryForCurrentUser
      .appending(path: "Library/Preferences", directoryHint: .isDirectory)
      .appending(path: domain + ".plist")
    defer {
      native.removePersistentDomain(forName: domain)
      native.synchronize()
      try? FileManager.default.removeItem(at: folder)
    }
    native.set(AppPreferences.format, forKey: AppPreferences.formatKey)
    native.set(false, forKey: key)
    #expect(native.synchronize())
    AppPreferences.register(native, domain: domain, file: file,
      backups: folder.appending(path: "Backups", directoryHint: .isDirectory))
    try AppPreferences.check(native)

    let context = PreferenceContext(native)
    let events = Mutex(0)
    let initial = withObservationTracking {
      context.read(key, fallback: true) { $0.bool(forKey: $1) }
    } onChange: { events.withLock { $0 += 1 } }
    #expect(!initial)

    let writer = Process()
    let errors = Pipe()
    writer.executableURL = URL(filePath: "/usr/bin/defaults")
    writer.arguments = ["write", domain, key, "-bool", "true"]
    writer.standardError = errors
    try writer.run()
    writer.waitUntilExit()
    let diagnostic = String(decoding: errors.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
    try #require(writer.terminationStatus == 0, "The isolated native writer failed: \(diagnostic)")

    // Poll only the observation signal. Reading or synchronizing defaults here could hide a
    // missing wakeup from the external process and make the displayed UI remain stale.
    try await eventuallyAsync("the native external writer invalidates key \(key)", timeout: 5) {
      events.withLock { $0 } > 0
    }
    #expect(context.read(key, fallback: false) { $0.bool(forKey: $1) })
    #expect(native.integer(forKey: AppPreferences.formatKey) == AppPreferences.format)
  }
}
