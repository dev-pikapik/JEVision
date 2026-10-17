import SwiftUI

struct AppCommands: Commands {
  let project: ProjectStore
  @Environment(\.openWindow) private var openWindow
  @FocusedValue(\.canvasZoom) private var zoom

  var body: some Commands {
    CommandGroup(replacing: .newItem) {
      Button("New Test…") { project.isCreatingTest = true }
        .keyboardShortcut("n")
        .disabled(project.status != .ready)
      Button("Open Project…") { project.choose() }
        .keyboardShortcut("o")
      Button("Show Project in Finder") { project.revealRoot() }
        .disabled(project.rootURL == nil)
    }
    CommandGroup(after: .toolbar) {
      Button("Zoom In") { zoom?.wrappedValue = Zoom.step(zoom?.wrappedValue ?? 1, up: true) }
        .keyboardShortcut("+")
        .disabled(zoom == nil)
      Button("Zoom Out") { zoom?.wrappedValue = Zoom.step(zoom?.wrappedValue ?? 1, up: false) }
        .keyboardShortcut("-")
        .disabled(zoom == nil)
      Button("Actual Size") { zoom?.wrappedValue = 1 }
        .keyboardShortcut("0")
        .disabled(zoom == nil)
      Divider()
    }
    // The default Help item searches a help book the app does not have.
    CommandGroup(replacing: .help) {
      Button("Welcome to JEVision") { openWindow(id: SceneID.onboarding) }
    }
  }
}
