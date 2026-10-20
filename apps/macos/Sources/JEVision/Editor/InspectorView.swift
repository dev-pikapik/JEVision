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
        BlockNotes(document: document, block: id)
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
  static func fields(of kind: BlockKind) -> Set<String> {
    switch kind {
    case .openApp: ["app", "resetState", "windowSize", "windowPosition"]
    case .openFile: ["files", "app"]
    case .click: ["target"]
    case .hover: ["target"]
    case .drag: ["from", "to"]
    case .scroll: ["direction", "amount", "unit", "at"]
    case .type: ["text", "into"]
    case .pressKeys: ["keys"]
    case .waitFor: ["until", "timeout"]
    case .pause: ["duration"]
    case .check: ["reference"]
    case .if: ["reference", "value", "elseIf"]
    case .repeat: ["times"]
    case .useData: ["file"]
    case .runTest: ["test", "inputs"]
    case .script: ["language", "code", "inputs", "outputs", "timeout"]
    case .command: ["command", "folder", "exitCode", "saveOutput", "timeout"]
    case .clipboard: ["text", "saveAs", "to", "timeout"]
    case .dateTime: ["saveAs", "format", "shift"]
    case .http: ["method", "url", "headers", "body", "expectStatus", "save", "timeout"]
    case .checkFile: ["path", "until", "timeout"]
    case .database: ["file", "query", "saveAs", "expect", "timeout"]
    }
  }

  /// Adds, removes or reorders conditions of the `if` block as one Undo step each.
  private func editConditions(_ change: (BlockKind) -> BlockKind) {
    document.endUndoStep()
    document.update(id, field: "elseIf") { $0 = change($0) }
    document.endUndoStep()
  }

  /// A binding that rebuilds the block from one changed value.
  func bind<T>(_ field: String, _ value: T, _ make: @escaping (T) -> BlockKind) -> Binding<T> {
    let document = document, id = id
    return Binding(get: { value }, set: { new in document.update(id, field: field) { $0 = make(new) } })
  }

  @ViewBuilder
  private func fields(_ kind: BlockKind, flow: Flow, context: FieldContext) -> some View {
    let values = InsertableValues(
      secrets: requiredSecrets(flow: flow), columns: DataColumns.all(in: flow, folder: context.folder),
      environment: project.environments.valueNames, variables: savedVariables(flow: flow))
    switch kind {
    case .openApp(let app, let resetState, let windowSize, let windowPosition, let next):
      let make = { (app: String?, resetState: Bool, size: WindowSize?, position: WindowPosition?) in
        BlockKind.openApp(app: app, resetState: resetState, windowSize: size, windowPosition: position, next: next)
      }
      Section {
        LabeledContent("App") {
          AppPicker(
            bundleID: bind("app", app) { make($0, resetState, windowSize, windowPosition) },
            noneTitle: Text("The app under test (\(AppCatalog.shared.name(of: flow.app)))"))
        }
        IssueList(issues: context.issues("app"))
        Toggle("Start Fresh", isOn: bind("resetState", resetState) { make(app, $0, windowSize, windowPosition) })
          .accessibilityHint(Text("Quits the app if it is running, then opens it without the windows it had last time."))
        Text("Quits the app if it is running, then opens it without the windows it had last time.")
          .font(.callout)
          .foregroundStyle(.secondary)
      }
      Section {
        Toggle(
          "Fixed Window Size",
          isOn: bind("windowSize", windowSize != nil) {
            make(app, resetState, $0 ? WindowSize(width: 1000, height: 700) : nil, windowPosition)
          })
        if let size = windowSize {
          PointsField(
            label: Text("Width"),
            value: bind("windowSize", Int(size.width)) {
              make(app, resetState, WindowSize(width: UInt32(clamping: $0), height: size.height), windowPosition)
            })
          PointsField(
            label: Text("Height"),
            value: bind("windowSize", Int(size.height)) {
              make(app, resetState, WindowSize(width: size.width, height: UInt32(clamping: $0)), windowPosition)
            })
        }
        IssueList(issues: context.issues("windowSize"))
        Toggle(
          "Fixed Window Position",
          isOn: bind("windowPosition", windowPosition != nil) {
            make(app, resetState, windowSize, $0 ? WindowPosition(x: 80, y: 80) : nil)
          })
        if let place = windowPosition {
          PointsField(
            label: Text("From Left"),
            value: bind("windowPosition", Int(place.x)) {
              make(app, resetState, windowSize, WindowPosition(x: Int32(clamping: $0), y: place.y))
            })
          PointsField(
            label: Text("From Top"),
            value: bind("windowPosition", Int(place.y)) {
              make(app, resetState, windowSize, WindowPosition(x: place.x, y: Int32(clamping: $0)))
            })
        }
        IssueList(issues: context.issues("windowPosition"))
        Text("A fixed size and position keep pictures of the window the same on every run.")
          .font(.callout)
          .foregroundStyle(.secondary)
      }

    case .openFile(let files, let app, let newInstance, let closeAfter, let next):
      let make = { (files: [String], app: String?, newInstance: Bool, closeAfter: Bool) in
        BlockKind.openFile(files: files, app: app, newInstance: newInstance, closeAfter: closeAfter, next: next)
      }
      Section {
        ForEach(Array(files.enumerated()), id: \.offset) { index, path in
          HStack(alignment: .firstTextBaseline) {
            VStack(alignment: .leading) {
              FilePathField(
                label: Text("File \(index + 1)"),
                path: bind("files", path) { new in
                  var all = files
                  all[index] = new
                  return make(all, app, newInstance, closeAfter)
                },
                types: [.item], context: context)
              IssueList(issues: context.issues("files.\(index)"))
            }
            Button("Remove", systemImage: "minus.circle") {
              document.update(id, field: "files") {
                var all = files
                all.remove(at: index)
                $0 = make(all, app, newInstance, closeAfter)
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
            $0 = make(files + added, app, newInstance, closeAfter)
          }
        }
        IssueList(issues: context.issues("files").filter { $0.param("field") == "files" })
      } header: {
        Text("Files")
      } footer: {
        Text("The test opens a fresh copy of each file, so the original stays as it is.")
          .foregroundStyle(.secondary)
      }
      Section {
        let openWith = bind("app", app) { make(files, $0, newInstance, closeAfter) }
        if BrowserPicker.areLinks(files) {
          BrowserPicker(bundleID: openWith)
        } else {
          LabeledContent("Open With") {
            AppPicker(bundleID: openWith, noneTitle: Text("Default App"))
          }
        }
        IssueList(issues: context.issues("app"))
        Toggle(isOn: bind("closeAfter", closeAfter) { make(files, app, newInstance, $0) }) {
          Text("Close after the test")
          Text("When the test ends, even if it fails, closes what this block opened.")
        }
        DisclosureGroup {
          Toggle(isOn: bind("newInstance", newInstance) { make(files, app, $0, closeAfter) }) {
            Text("Open in a new copy of the app")
          }
        } label: {
          Text("More")
        }
      }

    case .click(let target, let button, let double, let modifiers, let waitForChange, let next):
      Section {
        TargetEditor(
          target: bind("target", target) {
            .click(target: $0, button: button, double: double, modifiers: modifiers, waitForChange: waitForChange, next: next)
          },
          field: "target", context: context)
      }
      Section {
        Picker(selection: bind("button", button) {
          .click(target: target, button: $0, double: double, modifiers: modifiers, waitForChange: waitForChange, next: next)
        }) {
          Text("Left").tag(MouseButton.left)
          Text("Right").tag(MouseButton.right)
        } label: {
          Text("Button")
        }
        Toggle(isOn: bind("double", double) {
          .click(target: target, button: button, double: $0, modifiers: modifiers, waitForChange: waitForChange, next: next)
        }) {
          Text("Double-click")
        }
        ModifiersField(
          modifiers: bind("modifiers", modifiers) {
            .click(target: target, button: button, double: double, modifiers: $0, waitForChange: waitForChange, next: next)
          })
        DisclosureGroup {
          Toggle(isOn: bind("waitForChange", waitForChange) {
            .click(target: target, button: button, double: double, modifiers: modifiers, waitForChange: $0, next: next)
          }) {
            Text("Wait until the screen changes")
          }
        } label: {
          Text("More")
        }
      }

    case .hover(let target, let waitForChange, let next):
      Section {
        TargetEditor(
          target: bind("target", target) { .hover(target: $0, waitForChange: waitForChange, next: next) },
          field: "target", context: context)
      }
      Section {
        DisclosureGroup {
          Toggle(isOn: bind("waitForChange", waitForChange) { .hover(target: target, waitForChange: $0, next: next) }) {
            Text("Wait until the screen changes")
          }
        } label: {
          Text("More")
        }
      }

    case .drag(let from, let to, let button, let modifiers, let waitForChange, let next):
      Section {
        TargetEditor(
          target: bind("from", from) {
            .drag(from: $0, to: to, button: button, modifiers: modifiers, waitForChange: waitForChange, next: next)
          },
          field: "from", context: context)
      } header: {
        Text("Drag From")
      }
      Section {
        TargetEditor(
          target: bind("to", to) {
            .drag(from: from, to: $0, button: button, modifiers: modifiers, waitForChange: waitForChange, next: next)
          },
          field: "to", context: context)
      } header: {
        Text("Drop On")
      }
      Section {
        Picker(selection: bind("button", button) {
          .drag(from: from, to: to, button: $0, modifiers: modifiers, waitForChange: waitForChange, next: next)
        }) {
          Text("Left").tag(MouseButton.left)
          Text("Right").tag(MouseButton.right)
        } label: {
          Text("Button")
        }
        ModifiersField(
          modifiers: bind("modifiers", modifiers) {
            .drag(from: from, to: to, button: button, modifiers: $0, waitForChange: waitForChange, next: next)
          })
        DisclosureGroup {
          Toggle(isOn: bind("waitForChange", waitForChange) {
            .drag(from: from, to: to, button: button, modifiers: modifiers, waitForChange: $0, next: next)
          }) {
            Text("Wait until the screen changes")
          }
        } label: {
          Text("More")
        }
      }

    case .scroll(let direction, let amount, let unit, let at, let waitForChange, let next):
      Section {
        Picker(selection: bind("direction", direction) {
          .scroll(direction: $0, amount: amount, unit: unit, at: at, waitForChange: waitForChange, next: next)
        }) {
          Text(String(localized: "scroll.down", defaultValue: "Down", bundle: .app, locale: .app)).tag(ScrollDirection.down)
          Text(String(localized: "scroll.up", defaultValue: "Up", bundle: .app, locale: .app)).tag(ScrollDirection.up)
          Text(String(localized: "scroll.left", defaultValue: "Left", bundle: .app, locale: .app)).tag(ScrollDirection.left)
          Text(String(localized: "scroll.right", defaultValue: "Right", bundle: .app, locale: .app)).tag(ScrollDirection.right)
        } label: {
          Text("Direction")
        }
        LabeledContent("Amount") {
          HStack {
            TextField(
              value: bind("amount", Int(amount)) {
                .scroll(
                  direction: direction, amount: UInt32(clamping: max($0, 1)), unit: unit, at: at,
                  waitForChange: waitForChange, next: next)
              }, format: .number
            ) { Text("Amount") }
            .labelsHidden()
            .multilineTextAlignment(.trailing)
            .frame(width: 64)
            Stepper(
              value: bind("amount", Int(amount)) {
                .scroll(
                  direction: direction, amount: UInt32(clamping: $0), unit: unit, at: at,
                  waitForChange: waitForChange, next: next)
              }, in: 1...10_000
            ) { Text("Amount") }
            .labelsHidden()
          }
        }
        Picker(selection: bind("unit", unit) {
          .scroll(direction: direction, amount: amount, unit: $0, at: at, waitForChange: waitForChange, next: next)
        }) {
          Text("Lines").tag(ScrollUnit.lines)
          Text("Pixels").tag(ScrollUnit.pixels)
        } label: {
          Text("Unit")
        }
        IssueList(issues: context.issues("amount"))
      }
      Section {
        Toggle(isOn: bind("at", at != nil) {
          .scroll(
            direction: direction, amount: amount, unit: unit, at: $0 ? .image("") : nil, waitForChange: waitForChange,
            next: next)
        }) {
          Text("Over an element")
        }
        if let at {
          TargetEditor(
            target: bind("at", at) {
              .scroll(direction: direction, amount: amount, unit: unit, at: $0, waitForChange: waitForChange, next: next)
            },
            field: "at", context: context)
        }
      } footer: {
        if at == nil { Text("Without an element, the wheel turns over the middle of the window.") }
      }
      Section {
        DisclosureGroup {
          Toggle(isOn: bind("waitForChange", waitForChange) {
            .scroll(direction: direction, amount: amount, unit: unit, at: at, waitForChange: $0, next: next)
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
          values: values)
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

    case .pause(let durationMs, let next):
      Section {
        TimeoutField(
          milliseconds: bind("duration", durationMs) { .pause(durationMs: $0, next: next) }, label: Text("Duration"))
        IssueList(issues: context.issues("duration"))
      } footer: {
        Text("Time the run is paused by you doesn’t count.")
          .foregroundStyle(.secondary)
      }

    case .check(let reference, let next):
      Section {
        ReferenceEditor(
          reference: bind("reference", reference) { .check(reference: $0, next: next) },
          field: "reference", context: context)
      }

    case .if(let condition, let then, let elseIf, let otherwise):
      let conditions = [condition] + elseIf.map(\.condition)
      ForEach(conditions.indices, id: \.self) { index in
        let at = index == 0 ? "" : "elseIf.\(index - 1)."
        Section {
          ConditionEditor(
            condition: bind(at + (conditions[index].isScreen ? "reference" : "value"), conditions[index]) { new in
              guard index > 0 else { return .if(condition: new, then: then, elseIf: elseIf, otherwise: otherwise) }
              var cases = elseIf
              cases[index - 1].condition = new
              return .if(condition: condition, then: then, elseIf: cases, otherwise: otherwise)
            },
            at: at, context: context, values: values)
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
                Text("The test takes the way of the first condition that holds, and the Otherwise way when none does.")
              } else {
                Text("The test goes the Yes way when the condition holds, and the No way when it doesn’t.")
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

    case .runTest(let test, let inputs, let next):
      let make = { (test: String, inputs: [String: String]) in BlockKind.runTest(test: test, inputs: inputs, next: next) }
      let own = document.url.standardizedFileURL
      let tests = project.items.allTests.filter { $0.url.standardizedFileURL != own }
      let blocks = project.library.blocks.filter { $0.url?.standardizedFileURL != own }
      let block = blocks.first { $0.url.map { RelativePath.from(context.folder, to: $0) } == test }
      Section {
        Picker(selection: bind("test", test) { make($0, inputs) }) {
          if test.isEmpty { Text("Choose a Test").tag("") }
          ForEach(tests) { item in
            Text(item.parentPath.isEmpty ? item.name : item.parentPath + "/" + item.name)
              .tag(RelativePath.from(context.folder, to: item.url))
          }
          if !blocks.isEmpty {
            Section("Custom Blocks") {
              ForEach(blocks) { entry in
                if let url = entry.url {
                  Label(entry.name, systemImage: entry.symbol).tag(RelativePath.from(context.folder, to: url))
                }
              }
            }
          }
          if !test.isEmpty, block == nil, !tests.contains(where: { RelativePath.from(context.folder, to: $0.url) == test }) {
            Text(test).tag(test)
          }
        } label: {
          Text("Test")
        }
        IssueList(issues: context.issues("test"))
      }
      if let block {
        Section {
          let names = block.inputs + inputs.keys.filter { !block.inputs.contains($0) }.sorted()
          ForEach(names, id: \.self) { name in
            TextField(
              text: bind("inputs", inputs[name] ?? "") { value in
                var all = inputs
                all[name] = value
                return make(test, all)
              },
              prompt: Text("Value")
            ) {
              Text(verbatim: name)
            }
            IssueList(issues: context.issues("inputs.\(name)"))
          }
          if let url = block.url, let root = project.rootURL {
            Button("Edit Block") { project.selection = RelativePath.from(root, to: url) }
              .help(Text("Opens the custom block, so you can change its steps for every test that uses it."))
          }
        } header: {
          Text("Inputs")
        } footer: {
          if block.inputs.isEmpty {
            Text("This block asks for nothing.")
              .foregroundStyle(.secondary)
          } else {
            Text("Values can hold ${secret.NAME}, ${env.NAME} and ${data.COLUMN} of this test.")
              .foregroundStyle(.secondary)
          }
        }
      }

    case .script(let language, let code, let inputs, let outputs, let timeoutMs, let next):
      scriptFields(
        language: language, code: code, inputs: inputs, outputs: outputs, timeoutMs: timeoutMs, next: next,
        context: context, values: values)

    case .clipboard(let action, let next):
      clipboardFields(action: action, next: next, context: context, values: values)

    case .dateTime(let saveAs, let format, let shift, let next):
      dateTimeFields(saveAs: saveAs, format: format, shift: shift, next: next, context: context)

    case .command(let command, let folder, let exitCode, let saveOutput, let timeoutMs, let next):
      commandFields(
        command: command, folder: folder, exitCode: exitCode, saveOutput: saveOutput, timeoutMs: timeoutMs,
        next: next, context: context, values: values)

    case .http(let method, let url, let headers, let body, let expectStatus, let save, let timeoutMs, let next):
      httpFields(
        method: method, url: url, headers: headers, body: body, expectStatus: expectStatus, save: save,
        timeoutMs: timeoutMs, next: next, context: context, values: values)

    case .checkFile(let path, let until, let timeoutMs, let next):
      checkFileFields(path: path, until: until, timeoutMs: timeoutMs, next: next, context: context, values: values)

    case .database(let file, let query, let saveAs, let expect, let timeoutMs, let next):
      databaseFields(
        file: file, query: query, saveAs: saveAs, expect: expect, timeoutMs: timeoutMs, next: next,
        context: context, values: values)
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
