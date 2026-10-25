import AppKit
import JEVisionCore
import JEVisionHands
import SwiftUI
import UniformTypeIdentifiers

/// What a field needs to know about the block and test it edits.
struct FieldContext {
  let issues: [Issue]
  /// Folder of the test file: paths are relative to it.
  let folder: URL
  let projectRoot: URL?
  let document: FlowDocument
  /// Id of the block being edited.
  let block: String

  /// Problems with `field` or anything inside it, like `reference` for `reference.image`.
  func issues(_ field: String) -> [Issue] {
    issues.filter { issue in
      guard let f = issue.param("field") else { return false }
      return f == field || f.hasPrefix(field + ".")
    }
  }

  /// Names of the values blocks of the test save as `kind`, such as the pictures and areas a
  /// target or a reference can take as `${var.NAME}`.
  @MainActor
  func variables(of kind: ValueKind) -> [String] {
    var names = Set<String>()
    for block in document.flow?.blocks ?? [] {
      switch block.kind {
      case .script(_, _, _, let outputs, _, _):
        names.formUnion(outputs.filter { $0.kind == kind }.map(\.name))
      case .clipboard(.read(let saveAs, let content), _) where (content == .image ? .image : .text) == kind:
        names.insert(saveAs)
      case .ask(.isShown, let saveAs, _) where kind == .number, .ask(.find, let saveAs, _) where kind == .area:
        names.insert(saveAs)
      default:
        break
      }
    }
    return names.filter { !$0.isEmpty }.sorted()
  }
}

/// Problems shown right under the field they are about.
struct IssueList: View {
  @Environment(ProjectStore.self) private var project
  let issues: [Issue]

  var body: some View {
    ForEach(issues, id: \.self) { issue in
      let isError = issue.severity == .error
      Label {
        Text(IssueText.message(issue))
      } icon: {
        Image(systemName: isError ? "exclamationmark.octagon.fill" : "exclamationmark.triangle.fill")
      }
      .font(.callout)
      .foregroundStyle(isError ? .red : .orange)
      .fixedSize(horizontal: false, vertical: true)
      if let need = BlockNeed(issue) {
        Button("Install…") { project.installBlock(for: need) }
          .help(Text("Opens the sheet that installs the block this step needs."))
      }
    }
  }
}

/// A file chosen in Finder and stored relative to the test, or by its label, which keeps
/// naming the file when it is moved or renamed in the project.
struct FilePathField: View {
  let label: Text
  @Binding var path: String
  let types: [UTType]
  let context: FieldContext
  /// Whether a dark-mode variant of the file, such as `inbox.dark.png`, counts as the file.
  var appearanceVariants = false
  /// Names of the pictures blocks save, which can stand in for the file as `${var.NAME}`.
  var pictures: [String] = []
  /// Why the chosen file is named by its path, or why a label could not be mended, for the
  /// path the field had then.
  @State private var problem: (path: String, text: String)?
  /// Changes when a label gets its file back, so the field looks it up again.
  @State private var relinks = 0

  private var url: URL { RelativePath.resolve(path, in: context.folder) }
  private var isWebAddress: Bool { path.hasPrefix("http://") || path.hasPrefix("https://") }
  /// A path with a placeholder names its file only once a run fills it in.
  private var isPlaceholder: Bool { path.contains("${") }

  var body: some View {
    let _ = relinks
    let resource = RelativePath.resolution(path, in: context.folder)
    LabeledContent {
      HStack {
        Text(shown(resource))
          .foregroundStyle(path.isEmpty ? .secondary : .primary)
          .truncationMode(.middle)
          .lineLimit(1)
          .textSelection(.enabled)
        Button("Choose…", action: choose)
      }
    } label: {
      label
    }
    if !pictures.isEmpty {
      LabeledContent {
        Menu {
          ForEach(pictures, id: \.self) { name in
            Button(name) { path = "${var.\(name)}" }
          }
        } label: {
          Text("Choose")
        }
        .fixedSize()
        .accessibilityLabel(Text("Use a Saved Picture"))
        .help(Text("Takes the picture a block saves earlier in the run, such as a screenshot a script makes."))
      } label: {
        Text("Or a picture a block saves")
          .foregroundStyle(.secondary)
      }
    }
    if let problem, problem.path == path {
      Warning(text: Text(problem.text))
    }
    switch resource {
    case (let project, .detached(let file, let name))?:
      Warning(text: Text(FileLabels.sentence("resource.detached", ["name": name, "path": shown(resource)])))
      Button("Restore Label") { restoreLabel(file, in: project) }
    case (let project, .missing(let name))?:
      Warning(text: Text(FileLabels.sentence("resource.missing", ["name": name])))
      Button("Find File…") { find(in: project) }
    case .some:
      EmptyView()
    case nil:
      if !path.isEmpty, !isWebAddress, !isPlaceholder, path.first != "/" {
        if AppearanceImage.url(path, in: context.folder, dark: false, variants: appearanceVariants) == nil {
          Warning(text: Text("There’s no such file next to the test."))
        } else if let root = context.projectRoot, !RelativePath.isInside(url, folder: root) {
          Warning(text: Text("This file is outside the project, so other people won’t get it with the tests."))
        }
      }
    }
  }

  /// The file as the person knows it: its path, never its label.
  private func shown(_ resource: (project: URL, status: Resolution)?) -> String {
    guard !path.isEmpty else { return String(localized: "None") }
    if case (_, .missing(let name))? = resource { return name }
    return RelativePath.path(of: path, in: context.folder)
  }

  private func choose() {
    guard let chosen = pick(from: path.isEmpty || isWebAddress || isPlaceholder ? context.folder : url.deletingLastPathComponent())
    else { return }
    let named = context.document.reference(to: chosen)
    path = named.reference
    problem = named.problem.map { (named.reference, $0) }
  }

  private func pick(from folder: URL) -> URL? {
    let panel = NSOpenPanel()
    panel.allowedContentTypes = types
    panel.allowsMultipleSelection = false
    panel.directoryURL = folder
    return panel.runModal() == .OK ? panel.url : nil
  }

  private func restoreLabel(_ file: String, in project: URL) {
    do {
      try restoreResourceLabel(project: project.path(percentEncoded: false), reference: path, file: file)
      problem = nil
    } catch {
      problem = (path, FileLabels.message(error))
    }
    relinks += 1
  }

  /// Gives the label of a missing file to the file the person finds in its place.
  private func find(in project: URL) {
    guard let chosen = pick(from: context.folder) else { return }
    do {
      switch try relinkResource(project: project.path(percentEncoded: false), reference: path, file: chosen.path(percentEncoded: false)) {
      case .labeled: problem = nil
      case .useOther(let reference): path = reference
      }
    } catch {
      problem = (path, FileLabels.message(error))
    }
    relinks += 1
  }
}

/// A note in orange under a field, about something that will go wrong.
struct Warning: View {
  let text: Text

  var body: some View {
    Label { text } icon: { Image(systemName: "exclamationmark.triangle.fill") }
      .font(.callout)
      .foregroundStyle(.orange)
      .fixedSize(horizontal: false, vertical: true)
  }
}

/// The file of an image for the light or dark appearance, as runs choose it: the dark variant
/// in dark mode if there is one, else the image itself.
enum AppearanceImage {
  /// The file for `dark` mode, or `nil` if neither is there. With `variants`, the other
  /// appearance's file stands in when this one's is missing, as for a reference saved only in
  /// one appearance.
  static func url(_ path: String, in folder: URL, dark: Bool, variants: Bool = false) -> URL? {
    guard !path.isEmpty else { return nil }
    let light = RelativePath.path(of: path, in: folder)
    let darkFile = referenceDarkVariant(image: light)
    let order = dark ? [darkFile, light] : variants ? [light, darkFile] : [light]
    return order.map { RelativePath.resolve($0, in: folder) }.first { FileManager.default.fileExists(atPath: $0.path) }
  }
}

/// A picture of the chosen image, so it's clear what the block looks for.
struct ImagePreview: View {
  let path: String
  let context: FieldContext
  @Environment(\.colorScheme) private var colorScheme

  var body: some View {
    if let url = AppearanceImage.url(path, in: context.folder, dark: colorScheme == .dark),
      let image = NSImage(contentsOf: url)
    {
      Image(nsImage: image)
        .resizable()
        .scaledToFit()
        .frame(maxWidth: .infinity, maxHeight: 120)
        .clipShape(.rect(cornerRadius: 6))
        .accessibilityLabel(Text("Preview of \(BlockSummary.imageName(path))"))
    }
  }
}

/// Records one key combination: the next key pressed with its modifiers.
struct KeyRecorder: View {
  @Binding var keys: String
  @State private var monitor: Any?

  var body: some View {
    LabeledContent {
      Button(action: toggle) {
        if monitor != nil {
          Text("Press keys…")
        } else if keys.isEmpty {
          Text("Record Keys")
        } else {
          Text(verbatim: KeyCombo.display(keys)).monospaced()
        }
      }
      .accessibilityHint(Text("Then press the key combination. Escape cancels."))
    } label: {
      Text("Keys")
    }
    .onDisappear(perform: stop)
  }

  private func toggle() {
    if monitor != nil { return stop() }
    let keys = $keys
    monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
      let held = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
      MainActor.assumeIsolated {
        if event.keyCode == 53, held.intersection([.command, .shift, .option, .control]).isEmpty {
          stop()
        } else if let combo = KeyCombo.from(keyCode: event.keyCode, modifierFlags: held)
        {
          keys.wrappedValue = combo
          stop()
        }
      }
      return nil
    }
  }

  private func stop() {
    if let monitor { NSEvent.removeMonitor(monitor) }
    monitor = nil
  }
}

/// Text to type, with menus that insert secrets, environment values and data columns as
/// placeholders.
struct PlaceholderTextField: View {
  @Binding var text: String
  let values: InsertableValues
  var label = Text("Text")
  var prompt = Text("Text to type")
  var monospaced = false
  @State private var isNamingSecret = false
  @State private var secretName = ""

  var body: some View {
    TextField(text: $text, prompt: prompt, axis: .vertical) { label }
      .lineLimit(1...4)
      .font(monospaced ? .body.monospaced() : .body)
      .autocorrectionDisabled(monospaced)
    LabeledContent {
      Menu {
        Section("Secrets") {
          ForEach(values.secrets, id: \.self) { name in
            Button(name) { insert("secret", name) }
          }
          Button("New Secret…") {
            secretName = ""
            isNamingSecret = true
          }
        }
        if !values.environment.isEmpty {
          Section("Environment Values") {
            ForEach(values.environment, id: \.self) { name in
              Button(name) { insert("env", name) }
            }
          }
        }
        if !values.columns.isEmpty {
          Section("Data Columns") {
            ForEach(values.columns, id: \.self) { name in
              Button(name) { insert("data", name) }
            }
          }
        }
        if !values.variables.isEmpty {
          Section("Saved Values") {
            ForEach(values.variables, id: \.self) { name in
              Button(name) { insert("var", name) }
            }
          }
        }
      } label: {
        Text("Insert")
      }
      .fixedSize()
    } label: {
      Text("A secret or a value kept outside the test")
        .foregroundStyle(.secondary)
    }
    .alert("New Secret", isPresented: $isNamingSecret) {
      TextField("Name", text: $secretName)
      Button("Insert") {
        let name = secretName.replacing(/[^A-Za-z0-9_]/, with: "_")
        if !name.isEmpty { insert("secret", name) }
      }
      Button("Cancel", role: .cancel) {}
    } message: {
      Text("Passwords and keys go here by name. Set the value in the test details: it stays in the Keychain and never gets into the file.")
    }
  }

  private func insert(_ scope: String, _ name: String) {
    text += "${\(scope).\(name)}"
  }
}

/// How long to wait before giving up, in seconds.
struct TimeoutField: View {
  @Binding var milliseconds: UInt64
  var label = Text("Time Limit")

  private var seconds: Binding<Double> {
    Binding(
      get: { Double(milliseconds) / 1000 },
      set: { milliseconds = UInt64((min(max($0, 0.1), 3600) * 1000).rounded()) })
  }

  var body: some View {
    LabeledContent {
      HStack(spacing: 6) {
        TextField(value: seconds, format: .number.precision(.fractionLength(0...1))) { label }
          .labelsHidden()
          .multilineTextAlignment(.trailing)
          .frame(width: 64)
        Stepper(value: seconds, in: 1...3600, step: 1) { label }
          .labelsHidden()
        Text("seconds")
      }
    } label: {
      label
    }
  }
}

/// A whole number of points, such as the width of a window.
struct PointsField: View {
  let label: Text
  @Binding var value: Int

  var body: some View {
    LabeledContent {
      HStack(spacing: 6) {
        TextField(value: $value, format: .number.grouping(.never)) { label }
          .labelsHidden()
          .multilineTextAlignment(.trailing)
          .frame(width: 64)
        Text("points")
      }
    } label: {
      label
    }
  }
}

/// The keys held down while a block clicks, drags, hovers or scrolls.
struct ModifiersField: View {
  @Binding var modifiers: Modifiers

  var body: some View {
    LabeledContent {
      HStack(spacing: 4) {
        key("⌃", String(localized: "modifier.control", defaultValue: "Control", bundle: .app, locale: .app), \.control)
        key("⌥", String(localized: "modifier.option", defaultValue: "Option", bundle: .app, locale: .app), \.option)
        key("⇧", String(localized: "modifier.shift", defaultValue: "Shift", bundle: .app, locale: .app), \.shift)
        key("⌘", String(localized: "modifier.command", defaultValue: "Command", bundle: .app, locale: .app), \.command)
      }
    } label: {
      Text("Hold Keys")
    }
  }

  private func key(_ symbol: String, _ name: String, _ path: WritableKeyPath<Modifiers, Bool>) -> some View {
    Toggle(isOn: Binding(get: { modifiers[keyPath: path] }, set: { modifiers[keyPath: path] = $0 })) {
      Text(verbatim: symbol)
    }
    .toggleStyle(.button)
    .accessibilityLabel(Text(verbatim: name))
    .help(name)
  }
}

/// A fraction of the window or image, entered as a percentage.
struct PercentField: View {
  let label: Text
  @Binding var value: Double

  var body: some View {
    TextField(value: $value, format: .percent.precision(.fractionLength(0...1))) { label }
  }
}

/// A reference image and how closely the screen must match it, or what it must show.
struct ReferenceEditor: View {
  @Binding var reference: Reference
  let field: String
  let context: FieldContext
  @AppStorage(AISettings.providerKey) private var aiProvider = ""
  @Environment(\.openSettings) private var openSettings
  @State private var isEditing = false
  @State private var picture: ReferencePicture?
  @Environment(ProjectStore.self) private var project
  @Environment(\.colorScheme) private var colorScheme

  private enum Mode: Hashable { case exact, similar, meaning }

  private var mode: Binding<Mode> {
    Binding(
      get: {
        switch reference.compare {
        case .exact: .exact
        case .similar: .similar
        case .meaning: .meaning
        }
      },
      set: {
        switch $0 {
        case .exact: reference.compare = .exact
        case .similar: reference.compare = .similar(threshold: 0.95)
        case .meaning: if !isMeaning { reference.compare = .meaning(expect: "", model: "", provider: "", confidence: 0.5) }
        }
      })
  }

  private var isMeaning: Bool {
    if case .meaning = reference.compare { true } else { false }
  }

  private var meaning: (expect: String, model: String, provider: String, confidence: Double) {
    if case .meaning(let expect, let model, let provider, let confidence) = reference.compare {
      (expect, model, provider, confidence)
    } else {
      ("", "", "", 0.5)
    }
  }

  private var expectation: Binding<String> {
    Binding(
      get: { meaning.expect },
      set: {
        reference.compare = .meaning(expect: $0, model: meaning.model, provider: meaning.provider, confidence: meaning.confidence)
      })
  }

  private var model: Binding<String> {
    Binding(
      get: { meaning.model },
      set: {
        reference.compare = .meaning(expect: meaning.expect, model: $0, provider: meaning.provider, confidence: meaning.confidence)
      })
  }

  private var confidence: Binding<Double> {
    Binding(
      get: { meaning.confidence },
      set: {
        reference.compare = .meaning(
          expect: meaning.expect, model: meaning.model, provider: meaning.provider, confidence: ($0 * 100).rounded() / 100)
      })
  }

  /// A model belongs to its provider, so choosing another provider clears it.
  private var provider: Binding<String> {
    Binding(
      get: { meaning.provider },
      set: {
        guard $0 != meaning.provider else { return }
        reference.compare = .meaning(expect: meaning.expect, model: "", provider: $0, confidence: meaning.confidence)
      })
  }

  /// Checks by meaning work with the AI of Settings or with the one the project names.
  private var hasAI: Bool { !aiProvider.isEmpty || project.settings.aiScreen.map { !$0.provider.isEmpty } == true }

  /// The provider that answers the check: the block's, the project's, or the one of Settings.
  private var judgingProvider: String {
    let inherited = resolveAiChoice(settings: project.settings, task: .screen, block: AiChoice(provider: meaning.provider, model: ""))
    return inherited.provider.isEmpty ? aiProvider : inherited.provider
  }

  private var threshold: Binding<Double> {
    Binding(
      get: { if case .similar(let t) = reference.compare { t } else { 0.95 } },
      set: { reference.compare = .similar(threshold: ($0 * 100).rounded() / 100) })
  }

  var body: some View {
    let isSaved = reference.image.contains("${")
    let file = RelativePath.previewFile(of: reference.image, in: context.folder, dark: colorScheme == .dark)
    FilePathField(
      label: Text("Reference Image"), path: $reference.image, types: [.png], context: context, appearanceVariants: true,
      pictures: context.variables(of: .image))
    IssueList(issues: context.issues(field + ".image"))
    ReferenceThumbnail(picture: picture, reference: reference) { isEditing = true }
    LabeledContent {
      // Without an image the editor captures or chooses one first.
      Button(reference.image.isEmpty ? "Add Reference…" : "Edit Reference…") { isEditing = true }
        .disabled(isSaved)
        .sheet(isPresented: $isEditing) { ReferenceAreaEditor(reference: $reference, field: field, context: context) }
    } label: {
      if isSaved {
        Text("A saved picture is known only during the run, so its areas can’t be marked here.")
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      } else if !reference.image.isEmpty {
        AreaLegend(reference: reference)
      }
    }
    .task(id: "\(file?.path ?? "")#\(project.referenceChanges)") {
      picture = file.flatMap(ReferencePicture.init(url:))
    }
    IssueList(issues: context.issues(field + ".area") + context.issues(field + ".ignore") + context.issues(field + ".hide"))
    Picker(selection: mode) {
      Text("Exactly").tag(Mode.exact)
      Text("Similar").tag(Mode.similar)
      if hasAI || isMeaning { Text("By Meaning").tag(Mode.meaning) }
    } label: {
      Text("Match")
    }
    if isMeaning {
      TextField(text: expectation, prompt: Text("The cart shows 2 items"), axis: .vertical) { Text("Expected") }
        .lineLimit(2...5)
      Text("Describe what the screen should show, in plain words. An AI model looks at the screen and decides. The reference image is optional: with one, the model compares the screen with it.")
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      IssueList(issues: context.issues(field + ".expect"))
      AIChoiceFields(
        provider: provider, model: model,
        help: "Which AI judges this check. “From Project or Settings” uses the one the project names, or else the one chosen in Settings → AI.",
        providerIssues: context.issues(field + ".provider"), modelIssues: context.issues(field + ".model"))
      ConfidenceSlider(confidence: confidence, provider: judgingProvider)
      IssueList(issues: context.issues(field + ".confidence"))
    }
    if !hasAI {
      HStack(alignment: .firstTextBaseline) {
        Text(isMeaning ? "Checking by meaning needs an AI model." : "An AI model can also check the screen from a sentence like “The cart shows 2 items”.")
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
        Spacer(minLength: 4)
        Button("Set Up AI…") {
          Profile.current.defaults.set(SettingsTab.ai.rawValue, forKey: SettingsTab.key)
          openSettings()
        }
        .buttonStyle(.link)
      }
      .font(.callout)
    }
    if case .similar = reference.compare {
      SimilaritySlider(threshold: threshold)
    }
    IssueList(issues: context.issues(field + ".threshold") + context.issues(field + ".compare"))
  }
}

/// Which AI looks at the screen and its model. Left empty, they are the ones the project names,
/// or else the ones chosen in Settings → AI.
struct AIChoiceFields: View {
  @Binding var provider: String
  @Binding var model: String
  let help: LocalizedStringKey
  let providerIssues: [Issue]
  let modelIssues: [Issue]
  @AppStorage(AISettings.providerKey) private var aiProvider = ""
  @Environment(ProjectStore.self) private var project

  private var hasAI: Bool { !aiProvider.isEmpty || project.settings.aiScreen.map { !$0.provider.isEmpty } == true }

  /// The model the project or the settings choose for the provider, shown when the block keeps it.
  private var settingsModel: String {
    let inherited = resolveAiChoice(settings: project.settings, task: .screen, block: AiChoice(provider: provider, model: ""))
    if !inherited.model.isEmpty { return inherited.model }
    return AISettings.provider(inherited.provider.isEmpty ? aiProvider : inherited.provider)
      .map { AISettings.app.model(for: $0) } ?? ""
  }

  /// Why there is no model to show in the empty field; said under it, where the line can wrap.
  private var noModelReason: LocalizedStringKey {
    hasAI ? "No model chosen in Settings → AI" : "No AI chosen in Settings → AI"
  }

  var body: some View {
    Picker(selection: $provider) {
      Text("From Project or Settings").tag("")
      ForEach(AISettings.providers, id: \.id) { Text($0.name).tag($0.id) }
    } label: {
      Text("AI")
    }
    Text(help)
      .font(.callout)
      .foregroundStyle(.secondary)
      .fixedSize(horizontal: false, vertical: true)
    IssueList(issues: providerIssues)
    TextField(text: $model, prompt: settingsModel.isEmpty ? nil : Text(verbatim: settingsModel)) { Text("Model") }
    if settingsModel.isEmpty {
      Text(noModelReason)
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
    }
    Text("Leave empty to use the model the project or Settings → AI chooses. Another model of the same provider can suit a harder or a simpler check.")
      .font(.callout)
      .foregroundStyle(.secondary)
      .fixedSize(horizontal: false, vertical: true)
    IssueList(issues: modelIssues)
  }
}

/// Where to click: an element found by its picture, by JEV from a description or by
/// accessibility, a fixed point in the window, or the middle of an area.
struct TargetEditor: View {
  @Binding var target: Target
  let field: String
  let context: FieldContext
  @Environment(\.openSettings) private var openSettings
  /// JEV has an API key; read from the Keychain when a description is shown, not on every redraw.
  @State private var finderReady = true

  private enum Way { case look, description, element, position, area }

  /// The JEV route that finds elements, as the settings name it.
  private var finderName: String { AISettings.provider(AISettings.app.finderID())?.name ?? "JEV" }

  private var way: Binding<Way> {
    Binding(
      get: {
        switch target {
        case .image: .look
        case .description: .description
        case .element: .element
        case .point: .position
        case .area: .area
        }
      },
      set: {
        switch $0 {
        case .look: target = .image("")
        case .description: target = .description("")
        case .element: target = .element(ElementCheck.new.element)
        case .position: target = .point(Point(x: 0.5, y: 0.5))
        case .area: target = .area("")
        }
      })
  }

  private var area: Binding<String> {
    Binding(
      get: { if case .area(let area) = target { area } else { "" } },
      set: { target = .area($0) })
  }

  private var description: Binding<String> {
    Binding(
      get: { if case .description(let text) = target { text } else { "" } },
      set: { target = .description($0) })
  }

  private var element: Binding<ElementQuery> {
    Binding(
      get: { if case .element(let query) = target { query } else { ElementCheck.new.element } },
      set: { target = .element($0) })
  }

  private var image: Binding<String> {
    Binding(
      get: { if case .image(let path) = target { path } else { "" } },
      set: { target = .image($0) })
  }

  private func coordinate(_ path: WritableKeyPath<Point, Double>) -> Binding<Double> {
    Binding(
      get: { if case .point(let p) = target { p[keyPath: path] } else { 0 } },
      set: { value in
        if case .point(var p) = target {
          p[keyPath: path] = value
          target = .point(p)
        }
      })
  }

  var body: some View {
    Picker(selection: way) {
      Text("By Look").tag(Way.look)
      Text("By Description").tag(Way.description)
      Text("By Accessibility").tag(Way.element)
      Text("By Position").tag(Way.position)
      Text("By Area").tag(Way.area)
    } label: {
      Text("Find")
    }
    switch way.wrappedValue {
    case .look:
      FilePathField(label: Text("Image"), path: image, types: [.png], context: context, pictures: context.variables(of: .image))
      ImagePreview(path: image.wrappedValue, context: context)
    case .description:
      TextField(text: description, prompt: Text("The blue Save button"), axis: .vertical) { Text("Element") }
        .lineLimit(1...4)
      HStack(alignment: .firstTextBaseline) {
        Text(
          finderReady
            ? "Describe the element in plain words. \(finderName) finds it on the screen, and the pointer goes to its middle."
            : "Finding an element by its description needs \(finderName) to be set up."
        )
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
        if !finderReady {
          Spacer(minLength: 4)
          Button("Set Up JEV…") {
            Profile.current.defaults.set(SettingsTab.ai.rawValue, forKey: SettingsTab.key)
            openSettings()
          }
          .buttonStyle(.link)
        }
      }
      .font(.callout)
      .task {
        let ai = AISettings.app
        finderReady = AISettings.provider(ai.finderID()).map(ai.isSetUp) ?? false
      }
    case .element:
      ElementQueryEditor(query: element)
    case .position:
      PercentField(label: Text("From Left"), value: coordinate(\.x))
      PercentField(label: Text("From Top"), value: coordinate(\.y))
    case .area:
      TextField(text: area, prompt: Text(verbatim: "0.1,0.2,0.5,0.3")) { Text("Area") }
        .font(.body.monospaced())
        .autocorrectionDisabled()
      let areas = context.variables(of: .area)
      if !areas.isEmpty {
        LabeledContent {
          Menu {
            ForEach(areas, id: \.self) { name in
              Button(name) { area.wrappedValue = "${var.\(name)}" }
            }
          } label: {
            Text("Choose")
          }
          .fixedSize()
          .accessibilityLabel(Text("Use a Saved Area"))
        } label: {
          Text("Or an area a block saves")
            .foregroundStyle(.secondary)
        }
      }
      Text("Four fractions of the window from 0 to 1: from the left, from the top, the width and the height. The pointer goes to the middle of the area. A script can find the area and save it for this block.")
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
    }
    IssueList(issues: context.issues(field))
  }
}
