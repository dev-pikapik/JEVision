import AppKit
import JEVisionHands
import SwiftUI
import Testing

@testable import JEVision

/// Exercises the editor in an offscreen window, without sending input to the desktop.
@MainActor
@Suite(.enabled(if: ProcessInfo.processInfo.environment["JEVISION_SCALE"] == "1"), .serialized)
struct CanvasControlTests {
  @Test func hidingAndShowingTheMapUpdatesWithoutAnotherCanvasAction() async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: "jevision-map-control-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    let profile = Profile(folder: root)
    defer {
      profile.defaults.removePersistentDomain(forName: profile.defaultsDomain)
      try? FileManager.default.removeItem(at: root)
    }
    let file = root.appending(path: "Map.flow.json")
    try #"{"format":1,"name":"Map","app":"com.apple.TextEdit","start":"a","blocks":{"a":{"type":"pause","duration":"1s"}}}"#
      .write(to: file, atomically: true, encoding: .utf8)
    let document = FlowDocument(url: file)
    try #require(document.flow != nil)
    let host = NSHostingView(rootView: EditorView(document: document)
      .environment(ProjectStore(defaults: profile.defaults))
      .environment(RunCenter(defaults: profile.defaults))
      .environment(RecordCenter())
      .defaultAppStorage(profile.defaults))
    let window = NSWindow(contentRect: NSRect(x: -10000, y: -10000, width: 1000, height: 700),
      styleMask: [.titled, .resizable], backing: .buffered, defer: false)
    window.isReleasedWhenClosed = false
    window.contentView = host
    window.orderBack(nil)
    defer {
      window.contentView = nil
      window.close()
    }

    func pixels() -> Data? {
      host.layoutSubtreeIfNeeded()
      window.displayIfNeeded()
      let rect = NSRect(x: host.bounds.maxX - 232, y: (host.isFlipped ? host.bounds.maxY - 180 : host.bounds.minY), width: 232, height: 180)
      guard let bitmap = host.bitmapImageRepForCachingDisplay(in: rect) else { return nil }
      host.cacheDisplay(in: rect, to: bitmap)
      return bitmap.representation(using: .png, properties: [:])
    }
    try await Task.sleep(for: .milliseconds(300))
    func buttons(_ view: NSView) -> [NSButton] {
      ((view as? NSButton).map { [$0] } ?? []) + view.subviews.flatMap(buttons)
    }
    // The map control is the rightmost native button in this one-block editor.
    func mapButton() -> NSButton? {
      buttons(host).max { a, b in
        a.convert(a.bounds, to: host).midX < b.convert(b.bounds, to: host).midX
      }
    }
    let hideButton = try #require(mapButton())
    let initialY = hideButton.convert(hideButton.bounds, to: host).midY
    hideButton.performClick(nil)
    try await eventuallyAsync("the map collapses without forced layout", timeout: 3) {
      guard let button = mapButton(), button !== hideButton else { return false }
      return abs(button.convert(button.bounds, to: host).midY - initialY) > 80
    }
    #expect(profile.defaults.object(forKey: "flowShowsMinimap") as? Bool == false)
    let showButton = try #require(mapButton())
    let hidden = try #require(pixels())
    showButton.performClick(nil)
    try await eventuallyAsync("the map returns without forced layout", timeout: 3) {
      guard let button = mapButton(), button !== showButton else { return false }
      return abs(button.convert(button.bounds, to: host).midY - initialY) < 1
    }
    #expect(profile.defaults.bool(forKey: "flowShowsMinimap"))
    try await eventuallyAsync("the map returns immediately", timeout: 3) { pixels().map { $0 != hidden } ?? false }
  }
}
