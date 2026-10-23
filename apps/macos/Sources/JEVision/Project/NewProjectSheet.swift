import AppKit
import JEVisionCore
import SwiftUI

/// The New Project sheet: a new project, empty or a copy of the example or of another project,
/// or a copy of a repository. The project opens once it's made; an empty one goes on to record
/// its first test.
struct NewProjectSheet: View {
  /// A first test was made to record: the recording starts once the sheet is gone.
  var created: () -> Void = {}
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  @State private var model: NewProjectModel
  /// The person has typed in the name field, so an empty name is worth a word under it.
  @State private var nameEdited = false
  /// The empty project is made and open: the sheet offers to record its first test.
  @State private var recordsFirstTest = false
  @State private var app: String?
  @State private var testFailure: String?

  init(request: NewProjectRequest, created: @escaping () -> Void = {}) {
    self.created = created
    _model = State(initialValue: NewProjectModel(request, copied: request == .clone ? Self.copiedText() : nil))
  }

  var body: some View {
    Group {
      if recordsFirstTest {
        firstTest
      } else {
        form
      }
    }
    .onDisappear { model.cancelClone() }
  }

  // MARK: Project

  private var form: some View {
    @Bindable var model = model
    return VStack(alignment: .leading, spacing: 16) {
      Text("New Project")
        .font(.title2.bold())
        .accessibilityAddTraits(.isHeader)
      Picker("New Project", selection: $model.mode) {
        Text("New").tag(NewProjectModel.Mode.new)
        Text("Clone").tag(NewProjectModel.Mode.clone)
      }
      .pickerStyle(.segmented)
      .labelsHidden()
      Form {
        if model.mode == .clone { addressField }
        nameField
        wherePicker
        if model.mode == .new { starters }
      }
      .formStyle(.columns)
      .disabled(model.isWorking)
      VStack(alignment: .leading, spacing: 4) {
        if let target = model.target {
          Text("Creates the folder \(target)")
            .textSelection(.enabled)
        }
        if model.mode == .new, let history = model.history {
          historyLine(history)
        }
      }
      .font(.callout)
      .foregroundStyle(.secondary)
      .fixedSize(horizontal: false, vertical: true)
      HStack {
        if model.isWorking, model.mode == .clone {
          ProgressView().controlSize(.small)
          Text("Copying the repository…")
            .foregroundStyle(.secondary)
        }
        Spacer()
        Button("Cancel", role: .cancel) {
          if model.isWorking { model.cancelClone() } else { dismiss() }
        }
        .keyboardShortcut(.cancelAction)
        Button(model.mode == .clone ? "Clone" : "Create", action: make)
          .keyboardShortcut(.defaultAction)
          .disabled(!model.canCreate)
      }
    }
    .padding(20)
    .frame(width: 480)
    .task(id: model.location) { await model.lookAtHistory() }
  }

  private var addressField: some View {
    @Bindable var model = model
    return LabeledContent {
      VStack(alignment: .leading, spacing: 4) {
        HStack {
          TextField(text: $model.address, prompt: Text(verbatim: "https://github.com/team/tests.git")) { Text("Address") }
            .labelsHidden()
          // Reading the clipboard on its own would make macOS ask the person first.
          PasteButton(payloadType: String.self) { texts in
            guard let text = texts.first else { return }
            Task { @MainActor in model.paste(text) }
          }
          .labelStyle(.iconOnly)
          .help(Text("Paste the address you copied"))
        }
        if let failure = model.failure, failure.field == .address { note(failure) }
      }
    } label: {
      Text("Address")
    }
  }

  private var nameField: some View {
    LabeledContent {
      VStack(alignment: .leading, spacing: 4) {
        TextField(
          text: Binding(
            get: { model.name },
            set: {
              model.name = $0
              nameEdited = true
            }),
          prompt: Text("Shop")
        ) { Text("Name") }
        .labelsHidden()
        if let failure = model.failure, failure.field == .name {
          note(failure)
        } else if let problem = model.nameProblem, nameEdited || !model.name.trimmingCharacters(in: .whitespaces).isEmpty {
          Text(problem)
            .font(.callout)
            .foregroundStyle(.red)
            .fixedSize(horizontal: false, vertical: true)
        }
      }
    } label: {
      Text("Name")
    }
  }

  private var wherePicker: some View {
    Picker(
      selection: Binding<URL?>(
        get: { model.location },
        set: { folder in
          if let folder {
            model.location = folder
          } else {
            // After the menu has closed, so the panel isn't opened from inside it.
            Task { chooseLocation() }
          }
        })
    ) {
      ForEach(model.locations, id: \.self) { folder in
        Text(verbatim: model.title(of: folder)).tag(URL?.some(folder))
      }
      Divider()
      Text("Other…").tag(URL?.none)
    } label: {
      Text("Where")
    }
  }

  private var starters: some View {
    LabeledContent {
      VStack(spacing: 6) {
        StarterCard(
          symbol: "doc", color: .gray, title: Text("Blank"),
          summary: Text("No tests yet: you record the first one next."),
          isSelected: model.starter == .empty
        ) { model.choose(.empty) }
        StarterCard(
          symbol: "text.page", color: .orange, title: Text("With Example"),
          summary: model.example == nil
            ? Text("This copy of JEVision doesn’t include the example project.")
            : Text("Ready tests for TextEdit, to see how everything works."),
          isSelected: model.starter == .example
        ) { model.choose(.example) }
        .disabled(model.example == nil)
        StarterCard(
          symbol: "doc.on.doc", color: .blue, title: Text("Copy of Project…"),
          summary: model.copySource.map { Text(verbatim: ProjectStore.shortPath($0.path)) }
            ?? Text("Tests, blocks and settings of another project, without its history and runs."),
          isSelected: model.starter == .copy, perform: chooseProjectToCopy)
      }
    } label: {
      Text("Start From")
    }
  }

  private func historyLine(_ history: NewProjectHistory) -> Text {
    switch history {
    case .own: Text("Version history is on: the project starts as a Git repository with a ready .gitignore.")
    case .enclosing: Text("Version history is kept by the Git repository this folder is in.")
    case .unavailable: Text("No version history: Git isn’t installed.")
    }
  }

  private func note(_ failure: NewProjectModel.Failure) -> some View {
    VStack(alignment: .leading, spacing: 2) {
      Text(failure.message)
        .font(.callout)
        .foregroundStyle(.red)
      if !failure.detail.isEmpty {
        Text(verbatim: failure.detail)
          .font(.caption)
          .foregroundStyle(.secondary)
          .textSelection(.enabled)
      }
    }
    .fixedSize(horizontal: false, vertical: true)
  }

  // MARK: First test

  private var firstTest: some View {
    VStack(spacing: 0) {
      FirstTestStep(app: $app, myTests: nil, record: recordFirstTest)
      if let testFailure {
        Text(testFailure)
          .font(.callout)
          .foregroundStyle(.red)
          .padding(.horizontal, 32)
      }
      HStack {
        Spacer()
        Button("Later") { dismiss() }
          .keyboardShortcut(.cancelAction)
      }
      .padding([.horizontal, .bottom], 20)
    }
    .frame(width: 640, height: 560)
  }

  /// Makes the test for the chosen app in the new project; it is recorded once the sheet is gone.
  private func recordFirstTest() {
    guard let app else { return }
    Task {
      do {
        try await project.createTest(name: String(localized: "First Test"), app: app)
      } catch {
        testFailure = ProjectStore.message(error)
        return
      }
      created()
      dismiss()
    }
  }

  // MARK: Making

  /// Makes or copies the project and opens it, after the test that runs or is being recorded in
  /// the open one is stopped, if the person agrees.
  private func make() {
    Task {
      guard await project.readyToLeave() else { return }
      switch model.mode {
      case .new:
        guard let made = await model.create() else { return }
        project.openProject(URL(filePath: made.path, directoryHint: .isDirectory), test: model.firstTest(of: made))
        if model.starter == .empty {
          withAnimation(.snappy) { recordsFirstTest = true }
        } else {
          dismiss()
        }
      case .clone:
        guard let folder = await model.clone() else { return }
        project.openProject(folder)
        dismiss()
      }
    }
  }

  private func chooseLocation() {
    let panel = NSOpenPanel()
    panel.canChooseDirectories = true
    panel.canChooseFiles = false
    panel.canCreateDirectories = true
    panel.message = String(localized: "Choose the folder to put the project in")
    panel.prompt = String(localized: "Choose")
    panel.directoryURL = model.location
    guard panel.runModal() == .OK, let url = panel.url else { return }
    model.choose(location: url)
  }

  private func chooseProjectToCopy() {
    let panel = NSOpenPanel()
    panel.canChooseDirectories = true
    panel.canChooseFiles = false
    panel.message = String(localized: "Choose the project to copy")
    panel.prompt = String(localized: "Choose")
    panel.directoryURL = model.copySource ?? project.rootURL ?? model.location
    guard panel.runModal() == .OK, let url = panel.url else { return }
    model.copy(of: url)
  }

  /// The text on the clipboard, when the person lets JEVision read it without asking each time;
  /// otherwise the Paste button next to the address brings it in.
  private static func copiedText() -> String? {
    let board = NSPasteboard.general
    guard board.accessBehavior == .alwaysAllow else { return nil }
    return board.string(forType: .string)
  }
}

/// A choice of what a new project starts from, picked like a radio button.
private struct StarterCard: View {
  let symbol: String
  let color: Color
  let title: Text
  let summary: Text
  let isSelected: Bool
  let perform: () -> Void

  var body: some View {
    Button(action: perform) {
      HStack(spacing: 10) {
        IconTile(symbol: symbol, color: color, size: 28)
        VStack(alignment: .leading, spacing: 2) {
          title.font(.headline)
          summary
            .font(.callout)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        }
        Spacer(minLength: 8)
        Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
          .imageScale(.large)
          .foregroundStyle(isSelected ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
          .accessibilityHidden(true)
      }
      .padding(8)
      .background(isSelected ? AnyShapeStyle(.tint.opacity(0.12)) : AnyShapeStyle(.clear), in: .rect(cornerRadius: 10))
      .overlay {
        RoundedRectangle(cornerRadius: 10)
          .strokeBorder(isSelected ? AnyShapeStyle(.tint) : AnyShapeStyle(.quaternary), lineWidth: 1)
      }
      .contentShape(.rect(cornerRadius: 10))
    }
    .buttonStyle(.plain)
    .accessibilityAddTraits(isSelected ? .isSelected : [])
  }
}
