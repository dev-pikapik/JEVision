import AppKit
import SwiftUI
import Testing

@testable import JEVision

@MainActor
@Suite(.enabled(if: ProcessInfo.processInfo.environment["JEVISION_SCALE"] == "1"), .serialized)
struct WorkspaceChromeTests {
  @Test func nativeTrackingAreaCoversTheTitlebar() async throws {
    let host = NSHostingView(rootView: Color.blue.ignoresSafeArea().frame(width: 600, height: 400)
      .toolbar { Text("Controls") }.modifier(WorkspaceChrome()))
    let window = NSWindow(contentRect: NSRect(x: -10000, y: -10000, width: 600, height: 400),
      styleMask: [.titled, .resizable], backing: .buffered, defer: false)
    window.isReleasedWhenClosed = false
    window.contentView = host
    window.orderBack(nil)
    defer { window.contentView = nil; window.close() }
    try await Task.sleep(for: .milliseconds(300))
    let frame = try #require(window.contentView?.superview)
    let area = try #require(frame.trackingAreas.first {
      guard let owner = $0.owner else { return false }
      return String(describing: type(of: owner)) == "Hook"
    })
    let close = try #require(window.standardWindowButton(.closeButton))
    let button = close.convert(close.bounds, to: frame)
    #expect(area.rect.height > 0)
    #expect(area.rect.contains(CGPoint(x: frame.bounds.midX, y: button.midY)))
    #expect(area.options.contains(.mouseEnteredAndExited))
  }
}
