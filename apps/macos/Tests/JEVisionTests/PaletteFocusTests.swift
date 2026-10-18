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
    @Environment(\.openWindow) private var openWindow
    @Environment(\.openSettings) private var openSettings
    @FocusState private var canvasFocused: Bool

    var body: some View {
      Color.clear
        .focusable()
        .focused($canvasFocused)
        .onAppear { canvasFocused = true }
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

  private func settle() { RunLoop.main.run(until: Date().addingTimeInterval(0.3)) }

  @Test func typingGoesStraightIntoTheSearchAndEscapeCloses() throws {
    let project = ProjectStore()
    let window = NSWindow(
      contentRect: NSRect(x: -10000, y: -10000, width: 800, height: 600), styleMask: [.titled], backing: .buffered,
      defer: false)
    window.isReleasedWhenClosed = false
    window.contentView = NSHostingView(
      rootView: Host(
        project: project, runs: RunCenter(defaults: UserDefaults(suiteName: "PaletteFocusTests")!),
        records: RecordCenter()))
    window.orderFront(nil)
    defer { window.close() }
    settle()

    project.showsCommandPalette = true
    settle()
    let panel = try #require(window.childWindows?.first as? CommandPalettePanel)
    #expect(panel.firstResponder is NSTextView, "The search field has the keyboard")

    panel.cancelOperation(nil)
    settle()
    #expect(!project.showsCommandPalette)
    #expect(window.childWindows?.isEmpty ?? true)

    project.showsCommandPalette = true
    settle()
    let again = try #require(window.childWindows?.first as? CommandPalettePanel)
    again.resignKey()
    settle()
    #expect(!project.showsCommandPalette, "A click outside closes it")
    #expect(window.childWindows?.isEmpty ?? true)
  }
}
