import SwiftUI

struct AppCommands: Commands {
  let project: ProjectStore
  let runs: RunCenter
  let records: RecordCenter
  @Environment(\.openWindow) private var openWindow
  @FocusedValue(\.canvasZoom) private var zoom
  @FocusedValue(\.canvasDocument) private var canvas

  var body: some Commands {
    CommandGroup(after: .appSettings) {
      Button("Install Command Line Tool…") { openWindow(id: SceneID.commandLineTool) }
    }
    CommandGroup(replacing: .newItem) {
      Button("New Test…") { project.isCreatingTest = true }
        .keyboardShortcut("n")
        .disabled(project.status != .ready)
      Button("Open Project…") { project.choose() }
        .keyboardShortcut("o")
      Button("Show Project in Finder") { project.revealRoot() }
        .disabled(project.rootURL == nil)
    }
    CommandGroup(after: .pasteboard) {
      Button("Duplicate") { canvas?.duplicate() }
        .keyboardShortcut("d")
        .disabled(canvas?.picked.isEmpty ?? true)
      Button("Link to Another Block") {
        if canvas?.startLink() != true { NSSound.beep() }
      }
      .keyboardShortcut("l")
      .disabled(canvas?.selection == nil)
    }
    CommandGroup(after: .toolbar) {
      Button("Zoom In") { zoom.map { $0.set(Zoom.step($0.value, up: true)) } }
        .keyboardShortcut("+")
        .disabled(zoom.map { $0.value >= Zoom.range.upperBound } ?? true)
      Button("Zoom Out") { zoom.map { $0.set(Zoom.step($0.value, up: false)) } }
        .keyboardShortcut("-")
        .disabled(zoom.map { $0.value <= Zoom.range.lowerBound } ?? true)
      Button("Zoom to Fit") { zoom?.fit() }
        .keyboardShortcut("0")
        .disabled(zoom == nil)
      Button("Actual Size") { zoom?.set(1) }
        .keyboardShortcut("0", modifiers: [.command, .option])
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
      Button("Run All Tests") {
        guard let root = project.rootURL else { return }
        runs.runAll(project.items.flatMap(\.tests).map(\.url), in: root, title: String(localized: "All tests"))
      }
      .keyboardShortcut("r", modifiers: [.command, .control])
      .disabled(runs.isRunning || records.isRecording || project.status != .ready || project.items.isEmpty)
      Divider()
      Button("Record") {
        if let document = project.document { records.record(document) }
      }
      .keyboardShortcut("r", modifiers: [.command, .shift])
      .disabled(!canRun)
    }
    // The default Help item searches a help book the app does not have.
    CommandGroup(replacing: .help) {
      Button("Welcome to JEVision") { openWindow(id: SceneID.onboarding) }
    }
  }

  private var canRun: Bool {
    !runs.isRunning && !records.isRecording && project.rootURL != nil && project.document?.flow != nil
  }

  private func run(stepByStep: Bool) {
    guard let root = project.rootURL, let document = project.document else { return }
    runs.run(document, in: root, stepByStep: stepByStep)
  }
}
