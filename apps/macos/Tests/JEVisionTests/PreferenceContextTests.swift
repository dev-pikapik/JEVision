import AppKit
import Combine
import JEVisionHands
import Observation
import SwiftUI
import Synchronization
import Testing

@testable import JEVision

@MainActor
struct PreferenceContextTests {
  private struct Probe: View {
    @AppPreference("shown") private var shown = false
    var body: some View { (shown ? Color.white : Color.black).frame(width: 8, height: 8) }
  }

  private func light(_ renderer: ImageRenderer<some View>) throws -> Bool {
    let image = try #require(renderer.cgImage)
    var pixel = [UInt8](repeating: 0, count: 4)
    try pixel.withUnsafeMutableBytes { bytes in
      let context = try #require(CGContext(data: bytes.baseAddress, width: 1, height: 1,
        bitsPerComponent: 8, bytesPerRow: 4, space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue))
      context.draw(image, in: CGRect(x: 0, y: 0, width: 1, height: 1))
    }
    return pixel[0] > 240
  }

  private func write(_ shown: Bool, to file: URL, header: Int = 1) throws {
    try PropertyListSerialization.data(fromPropertyList: [AppPreferences.formatKey: header, "shown": shown],
      format: .binary, options: 0).write(to: file, options: .atomic)
  }

  @Test func persistedProfileChangesInvalidateTheRenderedViewWithoutAnotherAction() async throws {
    let folder = FileManager.default.temporaryDirectory.appending(path: "jevision-preference-render-" + UUID().uuidString)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: folder) }
    let file = folder.appending(path: "Settings.plist")
    try write(false, to: file)
    let profile = Profile(folder: folder)
    let renderer = ImageRenderer(content: Probe().appPreferences(profile.defaults))
    renderer.scale = 1
    let events = Mutex(0)
    let token = renderer.objectWillChange.sink { _ in events.withLock { $0 += 1 } }
    defer { token.cancel() }
    #expect(try !light(renderer))

    var phase = 0
    func changes(after count: Int) async throws {
      phase += 1
      try await eventuallyAsync("preference render phase \(phase) receives its actual change event", timeout: 3) {
        events.withLock { $0 } > count
      }
    }
    var count = events.withLock { $0 }
    #expect(AppPreferences.set(true, forKey: "shown", in: profile.defaults))
    try await changes(after: count)
    #expect(try light(renderer))

    // A different process atomically replaces the file; no view, focus or camera action occurs.
    count = events.withLock { $0 }
    let replacement = folder.appending(path: "replacement.plist")
    try write(false, to: replacement)
    let writer = Process()
    writer.executableURL = URL(filePath: "/bin/mv")
    writer.arguments = [replacement.path, file.path]
    try writer.run()
    writer.waitUntilExit()
    #expect(writer.terminationStatus == 0)
    try await changes(after: count)
    #expect(try !light(renderer))

    count = events.withLock { $0 }
    #expect(AppPreferences.set(true, forKey: "shown", in: profile.defaults))
    try await changes(after: count)
    #expect(try light(renderer))
    count = events.withLock { $0 }
    try FileManager.default.removeItem(at: file)
    try await changes(after: count)
    #expect(try !light(renderer), "deletion reads the declared default")
    count = events.withLock { $0 }
    try write(true, to: file)
    try await changes(after: count)
    #expect(try light(renderer), "the directory watch follows the recreated file")

    count = events.withLock { $0 }
    try FileManager.default.removeItem(at: folder)
    try await changes(after: count)
    #expect(try !light(renderer))
    count = events.withLock { $0 }
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    try write(true, to: file)
    try await changes(after: count)
    #expect(try light(renderer), "the surviving ancestor follows a recreated profile directory")
  }

  @Test func nativeDottedKeysObserveWritesThroughAnotherAdapter() async throws {
    let domain = "test.jevision.preference-context." + UUID().uuidString
    let native = try #require(UserDefaults(suiteName: domain))
    defer { native.removePersistentDomain(forName: domain) }
    native.set(false, forKey: "video.enabled")
    let context = PreferenceContext(native)
    let events = Mutex(0)
    withObservationTracking {
      _ = context.read("video.enabled", fallback: false) { $0.bool(forKey: $1) }
    } onChange: { events.withLock { $0 += 1 } }
    #expect(AppPreferences.set(true, forKey: "video.enabled", in: native))
    try await eventuallyAsync("native identity connects independently created adapters", timeout: 3) {
      events.withLock { $0 } == 1
    }
    #expect(context.read("video.enabled", fallback: false) { $0.bool(forKey: $1) })
  }
}
