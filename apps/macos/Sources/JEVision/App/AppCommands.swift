import JEVisionCore
import SwiftUI

struct AppCommands: Commands {
  let project: ProjectStore
  let runs: RunCenter
  let records: RecordCenter
  let updates: Updates
  @Environment(\.openWindow) private var openWindow
  @Environment(\.openURL) private var openURL
  @FocusedValue(\.canvasZoom) private var zoom
  @FocusedValue(\.canvasDocument) private var canvas
  @FocusedValue(\.blockPalette) private var blockPalette

  var body: some Commands {
    CommandGroup(replacing: .appInfo) {
      Group {
        Button("About JEVision") { openWindow(id: SceneID.about) }
        if updates.isAvailable {
          Button("Check for Updates…") { updates.check() }
            .disabled(!updates.canCheck)
        }
      }
      .appLocale()
    }
    CommandGroup(after: .appSettings) {
      Group {
        Button("Install Command Line Tool…") { openWindow(id: SceneID.commandLineTool) }
      }
      .appLocale()
    }
    CommandGroup(replacing: .newItem) {
      Group {
        Button("New Test…") { project.isCreatingTest = true }
          .keyboardShortcut(Shortcuts.newTest)
          .disabled(project.status != .ready)
        Button("New Project…") { project.newProject = .new }
          .keyboardShortcut(Shortcuts.newProject)
        Button("Open Project…") { project.choose() }
          .keyboardShortcut(Shortcuts.openProject)
        Menu("Open Recent") {
          let recent = project.openableRecent
          ForEach(recent, id: \.path) { entry in
            Button(project.menuTitle(entry)) { project.openRecent(entry.path) }
          }
          if !recent.isEmpty { Divider() }
          Button("Clear Menu") { project.clearRecent() }
            .disabled(!project.recent.contains { !$0.pinned })
        }
        Button("Clone Repository…") { project.newProject = .clone }
        Button("Close Project") { project.close() }
          .keyboardShortcut(Shortcuts.closeProject)
          .disabled(project.rootURL == nil)
        Button("Show Project in Finder") { project.revealRoot() }
          .disabled(project.rootURL == nil)
        Divider()
        Button("Install Block…") { project.installBlock() }
          .disabled(project.status != .ready)
        Button("Custom Blocks…") { project.showsCustomBlocks = true }
          .disabled(project.status != .ready)
        Divider()
        Button("Commit Changes…") { openWindow(id: SceneID.commit) }
          .keyboardShortcut(Shortcuts.commit)
          .disabled(git?.changes.isEmpty ?? true)
        Button("New Pull Request…") { project.git.isComposingPullRequest = true }
          .keyboardShortcut(Shortcuts.newPullRequest)
          .disabled(git?.github == nil || git?.branch == nil || git?.hasCommits != true || project.git.activity != nil)
        Button("Pull Requests") { openWindow(id: SceneID.pullRequests) }
          .disabled(git?.github == nil)
      }
      .appLocale()
    }
    CommandGroup(after: .pasteboard) {
      Group {
        Button("Duplicate") { canvas?.duplicate() }
          .keyboardShortcut("d")
          .disabled(canvas?.picked.isEmpty ?? true)
        Button("Link to Another Block") {
          if canvas?.startLink() != true { NSSound.beep() }
        }
        .keyboardShortcut("l")
        .disabled(canvas?.selection == nil)
        Button("Disconnect") {
          if let canvas, let id = canvas.selection { canvas.disconnect(id) }
        }
        .keyboardShortcut(Shortcuts.disconnect)
        .disabled(!canDisconnect)
      }
      .disabled(canvas?.isReadOnly == true)
      .appLocale()
    }
    CommandGroup(before: .toolbar) {
      Group {
        Button("Command Palette…") { project.showsCommandPalette.toggle() }
          .keyboardShortcut(Shortcuts.commandPalette)
        Divider()
      }
      .appLocale()
    }
    CommandGroup(after: .toolbar) {
      Group {
        Button("Zoom In") { zoom.map { $0.set(Zoom.step($0.value, up: true, in: $0.range)) } }
          .keyboardShortcut("+")
          .disabled(zoom.map { $0.value >= $0.range.upperBound } ?? true)
        Button("Zoom Out") { zoom.map { $0.set(Zoom.step($0.value, up: false, in: $0.range)) } }
          .keyboardShortcut("-")
          .disabled(zoom.map { $0.value <= $0.range.lowerBound } ?? true)
        Button("Zoom to Fit") { zoom?.fit() }
          .keyboardShortcut("0")
          .disabled(zoom == nil)
        Button("Actual Size") { zoom?.set(1) }
          .keyboardShortcut("0", modifiers: [.command, .option])
          .disabled(zoom == nil)
        Divider()
      }
      .appLocale()
    }
    CommandMenu(String(localized: "Test")) {
      Group {
        Button("Run") { run(stepByStep: false) }
          .keyboardShortcut(Shortcuts.run)
          .disabled(!canRun)
        Button("Run Step by Step") { run(stepByStep: true) }
          .keyboardShortcut(Shortcuts.runStepByStep)
          .disabled(!canRun)
        Button("Run from Selected Block") { run(from: project.document?.selection) }
          .keyboardShortcut(Shortcuts.runFromSelection)
          .disabled(!canRun || project.document?.selection == nil)
        Button("Run All Tests") {
          guard let root = project.rootURL else { return }
          runs.runAll(project.chosen(from: project.items).map(\.url), in: root, title: project.runAllTitle)
        }
        .keyboardShortcut(Shortcuts.runAll)
        .disabled(runs.isBusy || records.isRecording || project.status != .ready || project.chosen(from: project.items).isEmpty)
        Button("Schedules…") { project.schedules.isShowingSheet = true }
          .disabled(project.status != .ready)
        Divider()
        Button("Record") {
          if let document = project.document { records.record(document) }
        }
        .keyboardShortcut(Shortcuts.record)
        .disabled(!canRun || project.document?.isReadOnly == true)
        Divider()
        Button("Add Block…") { blockPalette?.open() }
          .keyboardShortcut(Shortcuts.addBlock)
          .disabled(blockPalette == nil || project.document?.isReadOnly == true)
        Button { project.document?.addCondition() } label: {
          selectionIsIf ? Text("Add Condition") : Text("Add If")
        }
          .keyboardShortcut(Shortcuts.addCondition)
          .disabled(project.document?.flow == nil || project.document?.isReadOnly == true)
      }
      .appLocale()
    }
    // The Help menu itself, with its search of the menus, stays the system's. Its default item opens
    // a help book the app does not have, so the guides and the welcome window take its place.
    CommandGroup(replacing: .help) {
      Group {
        Button("Welcome to JEVision") { openWindow(id: SceneID.onboarding) }
        Divider()
        Button("Getting Started") { openURL(Guide.gettingStarted.url()) }
        Button("Writing Tests") { openURL(Guide.writingTests.url()) }
        Button("Checks") { openURL(Guide.checks.url()) }
        Button("Questions and Answers") { openURL(Guide.faq.url()) }
        Divider()
        Button("All Documentation") { openURL(Guide.contents.url()) }
      }
      .appLocale()
    }
  }

  private var canRun: Bool {
    !runs.isBusy && !records.isRecording && project.rootURL != nil && project.document?.flow != nil
      && project.document?.beforeDraft == nil
  }

  private var git: GitStatus? { project.git.status }

  private var canDisconnect: Bool {
    guard let canvas, let id = canvas.selection else { return false }
    return canvas.hasLinks(id)
  }

  private var selectionIsIf: Bool {
    guard let document = project.document, let id = document.selection else { return false }
    return document.block(id)?.kind.typeName == "if"
  }

  private func run(from block: String? = nil, stepByStep: Bool = false) {
    guard let root = project.rootURL, let document = project.document else { return }
    runs.run(document, in: root, from: block, stepByStep: stepByStep)
  }
}
