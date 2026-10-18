import Foundation
import Testing

/// A newcomer must see no macOS prompt before pressing a button that asks for one: the app only
/// checks permissions with calls that never prompt, asks in `Permission.request()` alone, and
/// leaves taking pictures of the screen to the runner.
struct PermissionsTests {
  private static let app = URL(filePath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .appending(path: "Sources/JEVision")

  private static func sources() throws -> [(name: String, text: String)] {
    let files = FileManager.default.enumerator(at: app, includingPropertiesForKeys: nil)?
      .compactMap { $0 as? URL }.filter { $0.pathExtension == "swift" } ?? []
    return try files.map { ($0.path.replacingOccurrences(of: app.path + "/", with: ""), try String(contentsOf: $0, encoding: .utf8)) }
  }

  @Test func onlyTheAllowButtonAsksMacOS() throws {
    let prompting = ["CGRequestScreenCaptureAccess", "AXIsProcessTrustedWithOptions", "tccutil"]
    let askers = try Self.sources().filter { file in prompting.contains { file.text.contains($0) } }.map(\.name)
    #expect(askers == ["Permissions/Permission.swift"])
  }

  @Test func theAppNeverCapturesTheScreenItself() throws {
    let capturing = ["SCShareableContent", "SCScreenshotManager", "SCStream", "CGWindowListCreateImage", "CGDisplayCreateImage"]
    let capturers = try Self.sources().filter { file in capturing.contains { file.text.contains($0) } }.map(\.name)
    #expect(capturers.isEmpty)
  }
}
