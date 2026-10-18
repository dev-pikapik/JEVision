import JEVisionCore
import SwiftUI
import UniformTypeIdentifiers

/// Settings of the selected block, a control for each.
struct InspectorView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(RunCenter.self) private var runs
  let document: FlowDocument
  let id: String
  /// How the block failed in the last run, when it did.
  var failure: BlockFailure?

  var body: some View {
    if let block = document.block(id), let flow = document.flow {
      let context = FieldContext(
        issues: document.issues(of: id), folder: document.url.deletingLastPathComponent(), projectRoot: project.rootURL,
        document: document, block: id)
      let type = block.kind.typeName
      Form {
        if let failure {
          failureBanner(failure)
        }
        Section {
          HStack(alignment: .top, spacing: 12) {
            IconTile(symbol: BlockInfo.symbol(type), color: BlockInfo.color(ofType: type), size: 36)
            VStack(alignment: .leading, spacing: 2) {
              Text(BlockInfo.title(type))
                .font(.title3.bold())
                .accessibilityAddTraits(.isHeader)
              Text(BlockInfo.explanation(type))
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            }
          }
          IssueList(issues: context.issues.filter { !Self.fields(of: block.kind).contains(field(of: $0)) })
        }
        fields(block.kind, flow: flow, context: context)
      }
      .formStyle(.grouped)
    }
  }

  private func failureBanner(_ failure: BlockFailure) -> some View {
    let reason = failure.failure.map(RunText.failure) ?? String(localized: "The block failed.")
    return Section {
      VStack(alignment: .leading, spacing: 10) {
        Label {
          VStack(alignment: .leading, spacing: 2) {
            Text("Failed in the last run")
              .font(.headline)
            Text(reason)
              .textSelection(.enabled)
              .fixedSize(horizontal: false, vertical: true)
          }
        } icon: {
          Image(systemName: "xmark.square.fill")
            .foregroundStyle(.red)
        }
        .accessibilityElement(children: .combine)
        HStack(spacing: 8) {
          Button("Open Report") { project.showRun(failure.run.url) }
            .buttonStyle(.glass)
          Button("Run Again", systemImage: "arrow.clockwise") { runAgain(failure.run.report) }
            .buttonStyle(.glass)
            .disabled(runs.isRunning || project.rootURL == nil || !project.hasTest(failure.run.report.file))
        }
      }
      .padding(.vertical, 4)
    }
    .listRowBackground(Color.red.opacity(0.12))
  }

  /// Runs the run's test again: this one, or the test that ran it.
  private func runAgain(_ report: RunReport) {
    guard let root = project.rootURL else { return }
    project.selection = report.file
    guard let document = project.document else { return }
    runs.run(document, in: root)
  }

  private func field(of issue: Issue) -> String {
    issue.param("field")?.split(separator: ".").first.map(String.init) ?? ""
  }

  /// Settings the inspector shows next to their own control; problems with anything else go to the top.
  private static func fields(of kind: BlockKind) -> Set<String> {
    switch kind {
    case .openApp: ["app"]
    case .openFile: ["files", "app"]
    case .click: ["target"]
    case .type: ["text", "into"]
    case .pressKeys: ["keys"]
    case .waitFor: ["until", "timeout"]
    case .check: ["reference"]
    case .if: ["reference", "elseIf"]
    case .repeat: ["times"]
    case .useData: ["file"]
    case .runTest: ["test"]
    }
  }

  /// Adds, removes or reorders conditions of the `if` block as one Undo step each.
  private func editConditions(_ change: (BlockKind) -> BlockKind) {
    document.endUndoStep()
    document.update(id, field: "elseIf") { $0 = change($0) }
    document.endUndoStep()
  }

  /// A binding that rebuilds the block from one changed value.
  private func bind<T>(_ field: String, _ value: T, _ make: @escaping (T) -> BlockKind) -> Binding<T> {
    let document = document, id = id
    return Binding(get: { value }, set: { new in document.update(id, field: field) { $0 = make(new) } })
  }

  @ViewBuilder
  private func fields(_ kind: BlockKind, flow: Flow, context: FieldContext) -> some View {
    switch kind {
    case .openApp(let app, let next):
      Section {
        LabeledContent("App") {
          AppPicker(
            bundleID: bind("app", app) { .openApp(app: $0, next: next) },
            noneTitle: Text("The app under test (\(AppCatalog.shared.name(of: flow.app)))"))
        }
        IssueList(issues: context.issues("app"))
      }

    case .openFile(let files, let app, let newInstance, let next):
      Section("Files") {
        ForEach(Array(files.enumerated()), id: \.offset) { index, path in
          HStack(alignment: .firstTextBaseline) {
            VStack(alignment: .leading) {
              FilePathField(
                label: Text("File \(index + 1)"),
                path: bind("files", path) { new in
                  var all = files
                  all[index] = new
                  return .openFile(files: all, app: app, newInstance: newInstance, next: next)
                },
                types: [.item], context: context)
              IssueList(issues: context.issues("files.\(index)"))
            }
            Button("Remove", systemImage: "minus.circle") {
              document.update(id, field: "files") {
                var all = files
                all.remove(at: index)
                $0 = .openFile(files: all, app: app, newInstance: newInstance, next: next)
              }
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
          }
        }
        Button("Add File…") {
          let panel = NSOpenPanel()
          panel.allowsMultipleSelection = true
          panel.directoryURL = context.folder
          guard panel.runModal() == .OK else { return }
          let added = panel.urls.map { RelativePath.from(context.folder, to: $0) }
          document.update(id, field: "files") {
            $0 = .openFile(files: files + added, app: app, newInstance: newInstance, next: next)
          }
        }
        IssueList(issues: context.issues("files").filter { $0.param("field") == "files" })
      }
      Section {
        LabeledContent("Open With") {
          AppPicker(
            bundleID: bind("app", app) { .openFile(files: files, app: $0, newInstance: newInstance, next: next) },
            noneTitle: Text("Default App"))
        }
        IssueList(issues: context.issues("app"))
        DisclosureGroup {
          Toggle(isOn: bind("newInstance", newInstance) { .openFile(files: files, app: app, newInstance: $0, next: next) }) {
            Text("Open in a new copy of the app")
          }
        } label: {
          Text("More")
        }
      }

    case .click(let target, let button, let double, let waitForChange, let next):
      Section {
        TargetEditor(
          target: bind("target", target) {
            .click(target: $0, button: button, double: double, waitForChange: waitForChange, next: next)
          },
          field: "target", context: context)
      }
      Section {
        Picker(selection: bind("button", button) {
          .click(target: target, button: $0, double: double, waitForChange: waitForChange, next: next)
        }) {
          Text("Left").tag(MouseButton.left)
          Text("Right").tag(MouseButton.right)
        } label: {
          Text("Button")
        }
        Toggle(isOn: bind("double", double) {
          .click(target: target, button: button, double: $0, waitForChange: waitForChange, next: next)
        }) {
          Text("Double-click")
        }
        DisclosureGroup {
          Toggle(isOn: bind("waitForChange", waitForChange) {
            .click(target: target, button: button, double: double, waitForChange: $0, next: next)
          }) {
            Text("Wait until the screen changes")
          }
        } label: {
          Text("More")
        }
      }

    case .type(let text, let into, let next):
      Section {
        PlaceholderTextField(
          text: bind("text", text) { .type(text: $0, into: into, next: next) },
          secrets: requiredSecrets(flow: flow), columns: DataColumns.all(in: flow, folder: context.folder),
          environmentValues: project.environments.valueNames)
        IssueList(issues: context.issues("text"))
      }
      Section {
        Toggle(isOn: bind("into", into != nil) { .type(text: text, into: $0 ? .image("") : nil, next: next) }) {
          Text("Click into a field first")
        }
        if let into {
          TargetEditor(
            target: bind("into", into) { .type(text: text, into: $0, next: next) }, field: "into", context: context)
        }
      }

    case .pressKeys(let keys, let next):
      Section {
        KeyRecorder(keys: bind("keys", keys) { .pressKeys(keys: $0, next: next) })
        IssueList(issues: context.issues("keys"))
      }

    case .waitFor(let until, let timeoutMs, let next):
      WaitForFields(
        until: bind("until", until) { .waitFor(until: $0, timeoutMs: timeoutMs, next: next) },
        context: context)
      Section {
        TimeoutField(milliseconds: bind("timeout", timeoutMs) { .waitFor(until: until, timeoutMs: $0, next: next) })
        IssueList(issues: context.issues("timeout"))
      }

    case .check(let reference, let next):
      Section {
        ReferenceEditor(
          reference: bind("reference", reference) { .check(reference: $0, next: next) },
          field: "reference", context: context)
      }

    case .if(let reference, let then, let elseIf, let otherwise):
      let conditions = [reference] + elseIf.map(\.reference)
      ForEach(conditions.indices, id: \.self) { index in
        let field = index == 0 ? "reference" : "elseIf.\(index - 1).reference"
        Section {
          ReferenceEditor(
            reference: bind(field, conditions[index]) { new in
              guard index > 0 else { return .if(reference: new, then: then, elseIf: elseIf, otherwise: otherwise) }
              var cases = elseIf
              cases[index - 1].reference = new
              return .if(reference: reference, then: then, elseIf: cases, otherwise: otherwise)
            },
            field: field, context: context)
          IssueList(issues: context.issues(field).filter { $0.param("field") == field })
          if conditions.count > 1 {
            HStack {
              Button("Move Up", systemImage: "arrow.up") {
                editConditions { moveIfCondition(kind: $0, from: UInt32(index), to: UInt32(index - 1)) }
              }
                .disabled(index == 0)
              Button("Move Down", systemImage: "arrow.down") {
                editConditions { moveIfCondition(kind: $0, from: UInt32(index), to: UInt32(index + 1)) }
              }
                .disabled(index == conditions.count - 1)
              Spacer()
              Button("Remove Condition", systemImage: "minus.circle") {
                editConditions { removeIfCondition(kind: $0, index: UInt32(index)) }
              }
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
          }
        } header: {
          if conditions.count > 1 { Text("Condition \(index + 1)") }
        } footer: {
          if index == conditions.count - 1 {
            Group {
              if conditions.count > 1 {
                Text("The test takes the way of the first condition the screen matches, and the Otherwise way when it matches none.")
              } else {
                Text("The test goes the Yes way when the screen matches the image, and the No way when it doesn’t.")
              }
            }
            .foregroundStyle(.secondary)
          }
        }
      }
      Section {
        Button("Add Condition", systemImage: "plus") {
          editConditions { addIfCondition(kind: $0) }
        }
      } footer: {
        Text("Each condition gets its own way out on the canvas.")
          .foregroundStyle(.secondary)
      }

    case .repeat(let times, let body, let next):
      Section {
        LabeledContent("Times") {
          HStack {
            TextField(value: bind("times", Int(times)) { .repeat(times: UInt32(clamping: max($0, 1)), body: body, next: next) },
              format: .number) { Text("Times") }
              .labelsHidden()
              .multilineTextAlignment(.trailing)
              .frame(width: 64)
            Stepper(value: bind("times", Int(times)) { .repeat(times: UInt32(clamping: $0), body: body, next: next) },
              in: 1...10_000) { Text("Times") }
              .labelsHidden()
          }
        }
        IssueList(issues: context.issues("times"))
      }

    case .useData(let file, let body, let next):
      Section {
        FilePathField(
          label: Text("Data File"), path: bind("file", file) { .useData(file: $0, body: body, next: next) },
          types: [.commaSeparatedText], context: context)
        IssueList(issues: context.issues("file"))
        let columns = DataColumns.read(file, folder: context.folder)
        if !columns.isEmpty {
          LabeledContent("Columns") {
            Text(columns.formatted(.list(type: .and)))
              .textSelection(.enabled)
          }
        }
        DataTableButton(file: file, context: context)
      } footer: {
        Text("Use a column in Type Text blocks inside the loop with Insert.")
          .foregroundStyle(.secondary)
      }

    case .runTest(let test, let next):
      Section {
        let tests = project.items.allTests.filter { $0.url.standardizedFileURL != document.url.standardizedFileURL }
        Picker(selection: bind("test", test) { .runTest(test: $0, next: next) }) {
          if test.isEmpty { Text("Choose a Test").tag("") }
          ForEach(tests) { item in
            Text(item.parentPath.isEmpty ? item.name : item.parentPath + "/" + item.name)
              .tag(RelativePath.from(context.folder, to: item.url))
          }
          if !test.isEmpty, !tests.contains(where: { RelativePath.from(context.folder, to: $0.url) == test }) {
            Text(test).tag(test)
          }
        } label: {
          Text("Test")
        }
        IssueList(issues: context.issues("test"))
      }
    }
  }
}

private struct WaitForFields: View {
  @Binding var until: WaitUntil
  let context: FieldContext

  private enum Mode: Hashable { case appears, disappears, stable }

  private var mode: Binding<Mode> {
    Binding(
      get: {
        switch until {
        case .appears: .appears
        case .disappears: .disappears
        case .screenStable: .stable
        }
      },
      set: {
        let reference = reference.wrappedValue
        switch $0 {
        case .appears: until = .appears(reference)
        case .disappears: until = .disappears(reference)
        case .stable: until = .screenStable
        }
      })
  }

  private var reference: Binding<Reference> {
    Binding(
      get: {
        switch until {
        case .appears(let r), .disappears(let r): r
        case .screenStable: Reference(image: "", compare: .similar(threshold: 0.95), area: nil, ignore: [], hide: [])
        }
      },
      set: { new in
        switch until {
        case .appears: until = .appears(new)
        case .disappears: until = .disappears(new)
        case .screenStable: break
        }
      })
  }

  var body: some View {
    Section {
      Picker(selection: mode) {
        Text("An image appears").tag(Mode.appears)
        Text("An image disappears").tag(Mode.disappears)
        Text("The screen stops changing").tag(Mode.stable)
      } label: {
        Text("Wait Until")
      }
    }
    if mode.wrappedValue != .stable {
      Section {
        ReferenceEditor(
          reference: reference, field: mode.wrappedValue == .appears ? "until.appears" : "until.disappears",
          context: context)
      }
    }
  }
}

/// Column names of the CSV files a test reads, from their first line.
enum DataColumns {
  static func read(_ file: String, folder: URL) -> [String] {
    guard !file.isEmpty, !file.contains("${") else { return [] }
    return (try? readDataRows(path: RelativePath.resolve(file, in: folder).path(percentEncoded: false)))?.columns ?? []
  }

  static func all(in flow: Flow, folder: URL) -> [String] {
    var seen = Set<String>()
    return files(in: flow).flatMap { read($0, folder: folder) }.filter { seen.insert($0).inserted }
  }

  static func files(in flow: Flow) -> [String] {
    flow.blocks.compactMap { if case .useData(let file, _, _) = $0.kind, !file.isEmpty { file } else { nil } }
  }
}

extension [ProjectItem] {
  /// Every test, at any depth, in sidebar order.
  var allTests: [ProjectItem] {
    flatMap { $0.kind == .test ? [$0] : $0.children.allTests }
  }
}
