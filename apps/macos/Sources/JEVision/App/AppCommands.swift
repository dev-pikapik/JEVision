import SwiftUI

struct AppCommands: Commands {
  let project: ProjectStore
  let runs: RunCenter
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
    CommandMenu("Test") {
      Button("Run") { run(stepByStep: false) }
        .keyboardShortcut("r")
        .disabled(!canRun)
      Button("Run Step by Step") { run(stepByStep: true) }
        .keyboardShortcut("r", modifiers: [.command, .option])
        .disabled(!canRun)
    }
    // The default Help item searches a help book the app does not have.
    CommandGroup(replacing: .help) {
      Button("Welcome to JEVision") { openWindow(id: SceneID.onboarding) }
    }
  }

  private var canRun: Bool {
    !runs.isRunning && project.rootURL != nil && project.document?.flow != nil
  }

  private func run(stepByStep: Bool) {
    guard let root = project.rootURL, let document = project.document else { return }
    runs.run(document, in: root, stepByStep: stepByStep)
  }
}
