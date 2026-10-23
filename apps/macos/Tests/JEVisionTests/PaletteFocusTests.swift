import AppKit
import SwiftUI
import Testing

@testable import JEVision

/// The palette in a window kept off screen, without any input sent to the Mac.
@MainActor struct PaletteFocusTests {
  private struct Host: View {
    let project: ProjectStore
    let runs: RunCenter
    let records: RecordCenter
    let shown: () -> Void
    @Environment(\.openWindow) private var openWindow
    @Environment(\.openSettings) private var openSettings
    @FocusState private var canvasFocused: Bool

    var body: some View {
      Color.clear
        .focusable()
        .focused($canvasFocused)
        .onAppear {
          canvasFocused = true
          shown()
        }
        .background {
          CommandPalettePresenter(isPresented: project.showsCommandPalette) {
            project.showsCommandPalette = false
          } palette: {
            CommandPaletteView(openWindow: openWindow, openSettings: openSettings)
              .environment(project)
              .environment(runs)
              .environment(records)
          }
        }
    }
  }

  private final class Flag {
    var isSet = false
  }

  @Test func typingGoesStraightIntoTheSearchAndEscapeCloses() throws {
    let project = ProjectStore()
    let shown = Flag()
    let window = NSWindow(
      contentRect: NSRect(x: -10000, y: -10000, width: 800, height: 600), styleMask: [.titled], backing: .buffered,
      defer: false)
    window.isReleasedWhenClosed = false
    window.contentView = NSHostingView(
      rootView: Host(
        project: project, runs: RunCenter(defaults: UserDefaults(suiteName: "PaletteFocusTests")!),
        records: RecordCenter(), shown: { shown.isSet = true }))
    window.orderFront(nil)
    defer { window.close() }
    try eventually("the window shows the view") { shown.isSet }
    let palette = { window.childWindows?.first as? CommandPalettePanel }

    project.showsCommandPalette = true
    try eventually("the palette opens") { palette() != nil }
    let panel = try #require(palette())
    try eventually("the search field has the keyboard") { panel.firstResponder is NSTextView }

    panel.cancelOperation(nil)
    try eventually("escape closes the palette") { window.childWindows?.isEmpty ?? true }
    #expect(!project.showsCommandPalette)

    project.showsCommandPalette = true
    try eventually("the palette opens again") { palette() != nil }
    let again = try #require(palette())
    again.resignKey()
    try eventually("a click outside closes it") { window.childWindows?.isEmpty ?? true }
    #expect(!project.showsCommandPalette, "A click outside closes it")
  }
}
