import AppKit
import JEVisionCore
import JEVisionHands
import SwiftUI
import UniformTypeIdentifiers

/// The areas a project hides in every screenshot, drawn over a picture of an app's window. The
/// picture only helps to place them and isn't kept; every change is saved to the project's
/// settings file right away and can be undone.
struct ProjectHideEditor: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  @Environment(\.undoManager) private var windowUndoManager
  /// Stands in when the window has no undo manager, so Undo always works here.
  @State private var ownUndoManager = UndoManager()
  @State private var picture: ReferencePicture?
  @State private var selection: AreaID?
  @State private var isCapturing = false
  @State private var problem: (title: String, problem: ReferenceAreaEditor.Problem)?
  /// Kind of the last change, so that changes of one kind in a row, like nudges, undo together.
  @State private var lastChange: String?
  @FocusState private var canvasFocused: Bool

  private var undoManager: UndoManager { windowUndoManager ?? ownUndoManager }

  /// The project's areas as the hidden areas of a reference, the shape the area tools work on.
  private var areas: Reference {
    Reference(image: "", compare: .exact, area: nil, ignore: [], hide: project.settings.hide)
  }

  /// The app of the test open in the editor, offered first for a picture.
  private var testApp: String? {
    guard let app = project.document?.flow?.app, !app.isEmpty else { return nil }
    return app
  }

  var body: some View {
    VStack(spacing: 0) {
      header
      Divider()
      HStack(spacing: 0) {
        canvas
          .frame(maxWidth: .infinity, maxHeight: .infinity)
          .background(.quaternary.opacity(0.5))
        Divider()
        sidebar
          .frame(width: 290)
      }
      Divider()
      footer
    }
    .frame(minWidth: 880, idealWidth: 1120, minHeight: 580, idealHeight: 760)
    .onAppear { canvasFocused = true }
    .onExitCommand { dismiss() }
    .onReceive(NotificationCenter.default.publisher(for: .NSUndoManagerDidUndoChange)) { _ in lastChange = nil }
    .alert(
      problem?.title ?? "", isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } }),
      presenting: problem?.problem
    ) { problem in
      if problem.needsPermission {
        Button("Open System Settings") { Permission.screenRecording.openSettings() }
      }
      Button("OK", role: .cancel) {}
    } message: { problem in
      Text(problem.message)
    }
  }

  // MARK: Parts

  private var header: some View {
    HStack(spacing: 12) {
      VStack(alignment: .leading, spacing: 2) {
        Text("Areas Always Hidden")
          .font(.headline)
          .accessibilityAddTraits(.isHeader)
        Text("Painted over in every screenshot of this project, for everyone who runs its tests.")
          .font(.callout)
          .foregroundStyle(.secondary)
      }
      Spacer()
      if isCapturing {
        ProgressView()
          .controlSize(.small)
          .accessibilityLabel(Text("Taking a picture"))
      }
      Menu {
        pictureChoices
      } label: {
        Label(picture == nil ? "Add Picture" : "Replace Picture", systemImage: "photo.badge.arrow.down")
      }
      .fixedSize()
      .disabled(isCapturing)
    }
    .padding(.horizontal, 16)
    .padding(.vertical, 10)
  }

  @ViewBuilder
  private var pictureChoices: some View {
    if let testApp {
      Button("Capture the \(AppCatalog.shared.name(of: testApp)) Window") { capture(testApp) }
    }
    Menu("Capture the Window of…") {
      let apps = Self.runningApps()
      if apps.isEmpty {
        Text("No other app is open")
      }
      ForEach(apps, id: \.bundleID) { app in
        Button(app.name) { capture(app.bundleID) }
      }
    }
    Divider()
    Button("Choose a Picture…", action: chooseFile)
  }

  @ViewBuilder
  private var canvas: some View {
    if let picture {
      AreaCanvas(
        picture: picture, reference: areas, tool: .hide, selection: $selection, focus: $canvasFocused,
        label: Text("Picture of the window, \(picture.image.width) by \(picture.image.height) pixels"), change: change)
    } else {
      ContentUnavailableView {
        Label("No Picture", systemImage: "macwindow")
      } description: {
        Text("Capture the window of the app you test, or choose a picture of it, to draw the areas on. The picture only helps you place them; it isn’t saved.")
      } actions: {
        Menu {
          pictureChoices
        } label: {
          Text("Add Picture")
        }
        .fixedSize()
        .disabled(isCapturing)
      }
    }
  }

  private var sidebar: some View {
    Form {
      Section {
        Button("Add Area", action: addArea)
          .disabled(picture == nil)
      } header: {
        Text("Draw")
      } footer: {
        Text("Drag on the picture to draw an area. Drag an area to move it, its corners to resize it. Areas are measured in parts of the window, so they fit it at any size.")
          .foregroundStyle(.secondary)
      }

      Section {
        if project.settings.hide.isEmpty {
          Text("No areas yet")
            .foregroundStyle(.secondary)
        }
        ForEach(areas.areaIDs, id: \.self) { id in
          if let rect = areas[area: id] {
            AreaRow(
              id: id, rect: rect, picture: picture, isSelected: selection == id,
              select: {
                selection = id
                canvasFocused = true
              },
              delete: { remove(id) })
          }
        }
        if let selection, let rect = areas[area: selection], let picture {
          AreaPixelFields(rect: rect, frame: ImageFrame(pixels: picture.pixels, bounds: picture.pixels)) { new in
            change("field.\(selection)") { $0[area: selection] = new }
          }
        }
      } header: {
        Text("Areas")
      } footer: {
        Text(
          "Before a screenshot of this project is saved in a report or goes to an AI model or an assistant, these areas are painted over in grey. They’re saved under “hide” in \(projectSettingsFileName())."
        )
        .foregroundStyle(.secondary)
      }
    }
    .formStyle(.grouped)
  }

  private var footer: some View {
    HStack {
      Text("Arrow keys move the selected area, Delete removes it.")
        .font(.callout)
        .foregroundStyle(.secondary)
      Spacer()
      Button("Undo") { undoManager.undo() }
        .keyboardShortcut("z")
        .disabled(!undoManager.canUndo)
      Button("Redo") { undoManager.redo() }
        .keyboardShortcut("z", modifiers: [.command, .shift])
        .disabled(!undoManager.canRedo)
      Button("Done") { dismiss() }
        .keyboardShortcut(.cancelAction)
        .buttonStyle(.borderedProminent)
    }
    .padding(.horizontal, 16)
    .padding(.vertical, 10)
  }

  // MARK: Changes

  /// Saves a change to the areas. Changes of the same `kind` in a row undo as one step; a
  /// `nil` kind is always a step of its own.
  private func change(_ kind: String?, _ edit: (inout Reference) -> Void) {
    var r = areas
    edit(&r)
    let joinsLast = kind != nil && kind == lastChange
    lastChange = kind
    do {
      try project.setHiddenAreas(r.hide, undoManager: joinsLast ? nil : undoManager)
    } catch {
      problem = (
        String(localized: "Can’t change the project settings"),
        ReferenceAreaEditor.Problem(message: ProjectStore.message(error))
      )
    }
  }

  private func remove(_ id: AreaID) {
    change(nil) { $0.remove(id) }
    selection = nil
  }

  private func addArea() {
    guard let picture else { return }
    let frame = ImageFrame(pixels: picture.pixels, bounds: picture.pixels)
    change(nil) { selection = $0.add(.hide, frame.snapped(Rect(x: 0.35, y: 0.4, width: 0.3, height: 0.2))) }
    canvasFocused = true
  }

  // MARK: The picture

  /// Apps with a window that are running now, other than JEVision, by name.
  private static func runningApps() -> [(bundleID: String, name: String)] {
    NSWorkspace.shared.runningApplications
      .filter { $0.activationPolicy == .regular && $0.bundleIdentifier != Bundle.main.bundleIdentifier }
      .compactMap { app in app.bundleIdentifier.map { ($0, app.localizedName ?? $0) } }
      .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
  }

  private func chooseFile() {
    let panel = NSOpenPanel()
    panel.allowedContentTypes = [.image]
    panel.allowsMultipleSelection = false
    panel.directoryURL = project.rootURL
    guard panel.runModal() == .OK, let url = panel.url else { return }
    guard let chosen = ReferencePicture(url: url) else {
      problem = (
        String(localized: "The Picture Wasn’t Opened"),
        ReferenceAreaEditor.Problem(message: String(localized: "This file can’t be read as a picture."))
      )
      return
    }
    picture = chosen
  }

  private func capture(_ app: String) {
    isCapturing = true
    Task {
      let result = await Task.detached(priority: .userInitiated) { () -> Result<Screenshot, HandsError> in
        Result { () throws(HandsError) in try MacHands(input: InputWatcher(), isCancelled: { false }).capture(app: app) }
      }.value
      isCapturing = false
      switch result {
      case .success(let shot):
        picture = ReferencePicture(data: shot.png)
      case .failure(let error):
        problem = (
          String(localized: "The Picture Wasn’t Taken"),
          ReferenceAreaEditor.problem(error, app: AppCatalog.shared.name(of: app))
        )
      }
    }
  }
}
