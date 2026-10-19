import AppKit
import JEVisionCore
import SwiftUI

/// Values a text field can insert as placeholders: secrets, data columns, environment values and
/// values earlier blocks save.
struct InsertableValues {
  let secrets: [String]
  let columns: [String]
  let environment: [String]
  let variables: [String]
}

/// Settings of the blocks that don't look at the screen: scripts, commands, HTTP requests, files
/// and databases, and the value conditions of an `if`.
extension InspectorView {
  @ViewBuilder
  func scriptFields(
    language: ScriptLanguage, code: String, inputs: [NamedValue], outputs: [ScriptOutput], timeoutMs: UInt64,
    next: String?, context: FieldContext, values: InsertableValues
  ) -> some View {
    let make = { (language: ScriptLanguage, code: String, inputs: [NamedValue], outputs: [ScriptOutput], timeout: UInt64) in
      BlockKind.script(language: language, code: code, inputs: inputs, outputs: outputs, timeoutMs: timeout, next: next)
    }
    Section {
      Picker(selection: bind("language", language) { make($0, code, inputs, outputs, timeoutMs) }) {
        ForEach(BlockInfo.languages, id: \.self) { Text(BlockInfo.languageName($0)).tag($0) }
      } label: {
        Text("Language")
      }
      CodeEditor(text: bind("code", code) { make(language, $0, inputs, outputs, timeoutMs) }, label: String(localized: "Code"))
        .frame(minHeight: 160)
      IssueList(issues: context.issues("code"))
    } footer: {
      Text("To hand a value to later blocks, write a name=value line to the file in $JEV_OUTPUT, like: echo \"total=3\" >> \"$JEV_OUTPUT\"")
        .foregroundStyle(.secondary)
        .textSelection(.enabled)
    }
    Section {
      NamedValuesEditor(
        rows: bind("inputs", inputs) { make(language, code, $0, outputs, timeoutMs) },
        namePrompt: Text(verbatim: "NAME"), valuePrompt: Text("Value"), field: "inputs", context: context)
    } header: {
      Text("Inputs")
    } footer: {
      Text("Each input reaches the script as an environment variable. Values can use Insert placeholders, such as ${var.NAME}.")
        .foregroundStyle(.secondary)
    }
    Section {
      ScriptOutputsEditor(
        outputs: bind("outputs", outputs) { make(language, code, inputs, $0, timeoutMs) }, context: context)
    } header: {
      Text("Outputs")
    } footer: {
      Text("The script must write each output. Later blocks read it as ${var.NAME}.")
        .foregroundStyle(.secondary)
    }
    timeoutSection(timeoutMs, context: context) { make(language, code, inputs, outputs, $0) }
  }

  @ViewBuilder
  func commandFields(
    command: String, folder: String?, exitCode: Int32, saveOutput: String?, timeoutMs: UInt64, next: String?,
    context: FieldContext, values: InsertableValues
  ) -> some View {
    let make = { (command: String, folder: String?, exitCode: Int32, saveOutput: String?, timeout: UInt64) in
      BlockKind.command(
        command: command, folder: folder, exitCode: exitCode, saveOutput: saveOutput, timeoutMs: timeout, next: next)
    }
    Section {
      PlaceholderTextField(
        text: bind("command", command) { make($0, folder, exitCode, saveOutput, timeoutMs) }, values: values,
        label: Text("Command"), prompt: Text(verbatim: "ls ~/Downloads"), monospaced: true)
      IssueList(issues: context.issues("command"))
    } footer: {
      Text("Runs in zsh, as in Terminal, with your permissions.")
        .foregroundStyle(.secondary)
    }
    Section {
      FolderField(
        folder: bind("folder", folder) { make(command, $0, exitCode, saveOutput, timeoutMs) }, context: context)
      IssueList(issues: context.issues("folder"))
      LabeledContent("Expected Exit Code") {
        TextField(value: bind("exitCode", Int(exitCode)) { make(command, folder, Int32(clamping: $0), saveOutput, timeoutMs) },
          format: .number) { Text("Expected Exit Code") }
          .labelsHidden()
          .multilineTextAlignment(.trailing)
          .frame(width: 64)
      }
      IssueList(issues: context.issues("exitCode"))
      OptionalNameField(
        label: Text("Save Output As"),
        name: bind("saveOutput", saveOutput) { make(command, folder, exitCode, $0, timeoutMs) })
      IssueList(issues: context.issues("saveOutput"))
    } footer: {
      Text("Exit code 0 means success. The saved output is what the command printed, without the last line break.")
        .foregroundStyle(.secondary)
    }
    timeoutSection(timeoutMs, context: context) { make(command, folder, exitCode, saveOutput, $0) }
  }

  @ViewBuilder
  func httpFields(
    method: HttpMethod, url: String, headers: [NamedValue], body: String, expectStatus: ExpectStatus,
    save: [SavedValue], timeoutMs: UInt64, next: String?, context: FieldContext, values: InsertableValues
  ) -> some View {
    let make = {
      (method: HttpMethod, url: String, headers: [NamedValue], body: String, expect: ExpectStatus, save: [SavedValue],
        timeout: UInt64) in
      BlockKind.http(
        method: method, url: url, headers: headers, body: body, expectStatus: expect, save: save, timeoutMs: timeout,
        next: next)
    }
    Section {
      Picker(selection: bind("method", method) { make($0, url, headers, body, expectStatus, save, timeoutMs) }) {
        ForEach(BlockInfo.methods, id: \.self) { Text(verbatim: BlockInfo.methodName($0)).tag($0) }
      } label: {
        Text("Method")
      }
      PlaceholderTextField(
        text: bind("url", url) { make(method, $0, headers, body, expectStatus, save, timeoutMs) }, values: values,
        label: Text("Address"), prompt: Text(verbatim: "https://api.example.com/items"), monospaced: true)
      IssueList(issues: context.issues("url"))
    }
    Section {
      NamedValuesEditor(
        rows: bind("headers", headers) { make(method, url, $0, body, expectStatus, save, timeoutMs) },
        namePrompt: Text(verbatim: "Authorization"), valuePrompt: Text("Value"), field: "headers", context: context)
    } header: {
      Text("Headers")
    }
    if method != .head {
      Section {
        PlaceholderTextField(
          text: bind("body", body) { make(method, url, headers, $0, expectStatus, save, timeoutMs) }, values: values,
          label: Text("Body"), prompt: Text(verbatim: "{\"name\": \"Ann\"}"), monospaced: true)
        IssueList(issues: context.issues("body"))
      } header: {
        Text("Body")
      } footer: {
        Text("A JSON body is sent as JSON unless a Content-Type header says otherwise.")
          .foregroundStyle(.secondary)
      }
    }
    Section {
      StatusField(status: bind("expectStatus", expectStatus) { make(method, url, headers, body, $0, save, timeoutMs) })
      IssueList(issues: context.issues("expectStatus"))
    }
    Section {
      NamedValuesEditor(
        rows: bind("save", save.map { NamedValue(name: $0.name, value: $0.from) }) {
          make(method, url, headers, body, expectStatus, $0.map { SavedValue(name: $0.name, from: $0.value) }, timeoutMs)
        },
        namePrompt: Text("Name"), valuePrompt: Text(verbatim: "body.items[0].id"), field: "save", context: context)
    } header: {
      Text("Save From the Answer")
    } footer: {
      Text("Take status, body, a field of a JSON body such as body.items[0].id, or a header such as header.Location. Later blocks read it as ${var.NAME}.")
        .foregroundStyle(.secondary)
    }
    timeoutSection(timeoutMs, context: context) { make(method, url, headers, body, expectStatus, save, $0) }
  }

  @ViewBuilder
  func checkFileFields(
    path: String, until: FileState, timeoutMs: UInt64, next: String?, context: FieldContext, values: InsertableValues
  ) -> some View {
    let make = { (path: String, until: FileState, timeout: UInt64) in
      BlockKind.checkFile(path: path, until: until, timeoutMs: timeout, next: next)
    }
    Section {
      OutsideFileField(
        label: Text("File"), path: bind("path", path) { make($0, until, timeoutMs) }, context: context)
      IssueList(issues: context.issues("path"))
    }
    Section {
      FileStateField(until: bind("until", until) { make(path, $0, timeoutMs) })
      IssueList(issues: context.issues("until"))
    } footer: {
      Text("“Changes” means the file was saved after the run started.")
        .foregroundStyle(.secondary)
    }
    timeoutSection(timeoutMs, context: context) { make(path, until, $0) }
  }

  @ViewBuilder
  func databaseFields(
    file: String, query: String, saveAs: String?, expect: DatabaseExpect?, timeoutMs: UInt64, next: String?,
    context: FieldContext, values: InsertableValues
  ) -> some View {
    let make = { (file: String, query: String, saveAs: String?, expect: DatabaseExpect?, timeout: UInt64) in
      BlockKind.database(file: file, query: query, saveAs: saveAs, expect: expect, timeoutMs: timeout, next: next)
    }
    Section {
      OutsideFileField(
        label: Text("SQLite File"), path: bind("file", file) { make($0, query, saveAs, expect, timeoutMs) },
        context: context)
      IssueList(issues: context.issues("file"))
      PlaceholderTextField(
        text: bind("query", query) { make(file, $0, saveAs, expect, timeoutMs) }, values: values,
        label: Text("Query"), prompt: Text(verbatim: "select count(*) from notes"), monospaced: true)
      IssueList(issues: context.issues("query"))
    }
    Section {
      OptionalNameField(
        label: Text("Save First Value As"), name: bind("saveAs", saveAs) { make(file, query, $0, expect, timeoutMs) })
      IssueList(issues: context.issues("saveAs"))
      DatabaseExpectField(expect: bind("expect", expect) { make(file, query, saveAs, $0, timeoutMs) })
      IssueList(issues: context.issues("expect"))
    } footer: {
      Text("The first value is the first column of the first row, empty when there are no rows. With an expected result, the query runs again until it holds or the time limit passes.")
        .foregroundStyle(.secondary)
    }
    timeoutSection(timeoutMs, context: context) { make(file, query, saveAs, expect, $0) }
  }

  private func timeoutSection(
    _ timeoutMs: UInt64, context: FieldContext, _ make: @escaping (UInt64) -> BlockKind
  ) -> some View {
    Section {
      TimeoutField(milliseconds: bind("timeout", timeoutMs, make))
      IssueList(issues: context.issues("timeout"))
    }
  }
}

/// Code typed as it is: monospaced, with no smart quotes, dashes or spelling fixes.
struct CodeEditor: NSViewRepresentable {
  @Binding var text: String
  let label: String

  func makeNSView(context: Context) -> NSScrollView {
    let scroll = NSTextView.scrollableTextView()
    scroll.borderType = .bezelBorder
    guard let view = scroll.documentView as? NSTextView else { return scroll }
    view.delegate = context.coordinator
    view.font = .monospacedSystemFont(ofSize: NSFont.systemFontSize, weight: .regular)
    view.isRichText = false
    view.allowsUndo = true
    view.isAutomaticQuoteSubstitutionEnabled = false
    view.isAutomaticDashSubstitutionEnabled = false
    view.isAutomaticTextReplacementEnabled = false
    view.isAutomaticSpellingCorrectionEnabled = false
    view.isContinuousSpellCheckingEnabled = false
    view.isAutomaticLinkDetectionEnabled = false
    view.textContainerInset = NSSize(width: 4, height: 6)
    view.setAccessibilityLabel(label)
    view.string = text
    return scroll
  }

  func updateNSView(_ scroll: NSScrollView, context: Context) {
    context.coordinator.text = $text
    guard let view = scroll.documentView as? NSTextView, view.string != text else { return }
    view.string = text
  }

  func makeCoordinator() -> Coordinator { Coordinator(text: $text) }

  final class Coordinator: NSObject, NSTextViewDelegate {
    var text: Binding<String>

    init(text: Binding<String>) { self.text = text }

    func textDidChange(_ notification: Notification) {
      guard let view = notification.object as? NSTextView else { return }
      text.wrappedValue = view.string
    }
  }
}

/// A list of name and value pairs, such as the inputs of a script or the headers of a request.
struct NamedValuesEditor: View {
  @Binding var rows: [NamedValue]
  let namePrompt: Text
  let valuePrompt: Text
  /// Field of the list in the file, such as `headers`; problems with a row are under it.
  let field: String
  let context: FieldContext

  var body: some View {
    ForEach(rows.indices, id: \.self) { index in
      HStack {
        TextField(text: binding(index, \.name), prompt: namePrompt) { Text("Name") }
          .labelsHidden()
          .font(.body.monospaced())
          .autocorrectionDisabled()
          .accessibilityLabel(Text("Name \(index + 1)"))
        TextField(text: binding(index, \.value), prompt: valuePrompt) { Text("Value") }
          .labelsHidden()
          .font(.body.monospaced())
          .autocorrectionDisabled()
          .accessibilityLabel(Text("Value \(index + 1)"))
        Button("Remove", systemImage: "minus.circle") { rows.remove(at: index) }
          .labelStyle(.iconOnly)
          .buttonStyle(.borderless)
      }
      IssueList(issues: context.issues("\(field).\(index)"))
    }
    Button("Add", systemImage: "plus") { rows.append(NamedValue(name: "", value: "")) }
    IssueList(issues: context.issues(field).filter { $0.param("field") == field })
  }

  private func binding(_ index: Int, _ path: WritableKeyPath<NamedValue, String>) -> Binding<String> {
    Binding(
      get: { index < rows.count ? rows[index][keyPath: path] : "" },
      set: { new in
        guard index < rows.count else { return }
        rows[index][keyPath: path] = new
      })
  }
}

/// What a script hands to later blocks: a name and the kind of value for each output.
struct ScriptOutputsEditor: View {
  @Binding var outputs: [ScriptOutput]
  let context: FieldContext

  var body: some View {
    ForEach(outputs.indices, id: \.self) { index in
      HStack {
        TextField(text: name(index), prompt: Text("Name")) { Text("Name") }
          .labelsHidden()
          .font(.body.monospaced())
          .autocorrectionDisabled()
          .accessibilityLabel(Text("Output \(index + 1)"))
        Picker(selection: kind(index)) {
          Text("Text").tag(OutputKind.text)
          Text("Number").tag(OutputKind.number)
          Text("True or False").tag(OutputKind.boolean)
        } label: {
          Text("Kind of Value")
        }
        .labelsHidden()
        .fixedSize()
        Button("Remove", systemImage: "minus.circle") { outputs.remove(at: index) }
          .labelStyle(.iconOnly)
          .buttonStyle(.borderless)
      }
      IssueList(issues: context.issues("outputs.\(index)"))
    }
    Button("Add Output", systemImage: "plus") { outputs.append(ScriptOutput(name: "", kind: .text)) }
  }

  private func name(_ index: Int) -> Binding<String> {
    Binding(
      get: { index < outputs.count ? outputs[index].name : "" },
      set: { if index < outputs.count { outputs[index].name = $0 } })
  }

  private func kind(_ index: Int) -> Binding<OutputKind> {
    Binding(
      get: { index < outputs.count ? outputs[index].kind : .text },
      set: { if index < outputs.count { outputs[index].kind = $0 } })
  }
}

/// A name to save a value as, or none.
struct OptionalNameField: View {
  let label: Text
  @Binding var name: String?

  var body: some View {
    TextField(
      text: Binding(get: { name ?? "" }, set: { name = $0.isEmpty ? nil : $0 }), prompt: Text("Don’t Save")
    ) { label }
      .font(.body.monospaced())
      .autocorrectionDisabled()
  }
}

/// Folder a command runs in, relative to the test; empty for the test's own folder.
struct FolderField: View {
  @Binding var folder: String?
  let context: FieldContext

  var body: some View {
    LabeledContent {
      HStack {
        Text(folder ?? String(localized: "The Test’s Folder"))
          .foregroundStyle(folder == nil ? .secondary : .primary)
          .truncationMode(.middle)
          .lineLimit(1)
          .textSelection(.enabled)
        Button("Choose…") {
          let panel = NSOpenPanel()
          panel.canChooseDirectories = true
          panel.canChooseFiles = false
          panel.directoryURL = context.folder
          guard panel.runModal() == .OK, let url = panel.url else { return }
          let path = RelativePath.from(context.folder, to: url)
          folder = path.isEmpty || path == "." ? nil : path
        }
        if folder != nil {
          Button("Clear", systemImage: "xmark.circle.fill") { folder = nil }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
        }
      }
    } label: {
      Text("Folder")
    }
  }
}

/// A file another app keeps: chosen in Finder and stored relative to the test inside the project,
/// from the home folder as `~/…`, or as a full path.
struct OutsideFileField: View {
  let label: Text
  @Binding var path: String
  let context: FieldContext

  var body: some View {
    TextField(text: $path, prompt: Text(verbatim: "~/Library/…")) { label }
      .font(.body.monospaced())
      .autocorrectionDisabled()
    LabeledContent {
      Button("Choose…") {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.directoryURL = context.folder
        guard panel.runModal() == .OK, let url = panel.url else { return }
        path = Self.path(of: url, context: context)
      }
    } label: {
      Text("A path inside the project, from the home folder (~/…) or a full path.")
        .foregroundStyle(.secondary)
    }
  }

  static func path(of url: URL, context: FieldContext) -> String {
    if let root = context.projectRoot, RelativePath.isInside(url, folder: root) {
      return RelativePath.from(context.folder, to: url)
    }
    let home = FileManager.default.homeDirectoryForCurrentUser.standardizedFileURL.path(percentEncoded: false)
    let full = url.standardizedFileURL.path(percentEncoded: false)
    let prefix = home.hasSuffix("/") ? home : home + "/"
    return full.hasPrefix(prefix) ? "~/" + full.dropFirst(prefix.count) : full
  }
}

/// Which HTTP status the answer must have.
struct StatusField: View {
  @Binding var status: ExpectStatus

  private enum Mode: Hashable { case success, any, exactly }

  private var mode: Binding<Mode> {
    Binding(
      get: {
        switch status {
        case .success: .success
        case .any: .any
        case .exactly: .exactly
        }
      },
      set: {
        switch $0 {
        case .success: status = .success
        case .any: status = .any
        case .exactly: status = .exactly(200)
        }
      })
  }

  var body: some View {
    Picker(selection: mode) {
      Text("Success (200–299)").tag(Mode.success)
      Text("Any Status").tag(Mode.any)
      Text("Exactly…").tag(Mode.exactly)
    } label: {
      Text("Expected Status")
    }
    if case .exactly(let code) = status {
      LabeledContent("Status Code") {
        TextField(value: Binding(get: { Int(code) }, set: { status = .exactly(UInt16(clamping: $0)) }), format: .number) {
          Text("Status Code")
        }
        .labelsHidden()
        .multilineTextAlignment(.trailing)
        .frame(width: 64)
      }
    }
  }
}

/// What a file should do before the test goes on.
struct FileStateField: View {
  @Binding var until: FileState

  private enum Mode: Hashable { case exists, missing, changed, contains }

  private var mode: Binding<Mode> {
    Binding(
      get: {
        switch until {
        case .exists: .exists
        case .missing: .missing
        case .changed: .changed
        case .contains: .contains
        }
      },
      set: {
        switch $0 {
        case .exists: until = .exists
        case .missing: until = .missing
        case .changed: until = .changed
        case .contains: until = .contains("")
        }
      })
  }

  var body: some View {
    Picker(selection: mode) {
      Text("The file appears").tag(Mode.exists)
      Text("The file disappears").tag(Mode.missing)
      Text("The file changes").tag(Mode.changed)
      Text("The file contains a text").tag(Mode.contains)
    } label: {
      Text("Wait Until")
    }
    if case .contains(let text) = until {
      TextField(text: Binding(get: { text }, set: { until = .contains($0) }), prompt: Text("Text to find")) {
        Text("Text")
      }
    }
  }
}

/// The result a database query should give.
struct DatabaseExpectField: View {
  @Binding var expect: DatabaseExpect?

  private enum Mode: Hashable { case nothing, value, rows }

  private var mode: Binding<Mode> {
    Binding(
      get: {
        switch expect {
        case nil: .nothing
        case .value: .value
        case .rows: .rows
        }
      },
      set: {
        switch $0 {
        case .nothing: expect = nil
        case .value: expect = .value("")
        case .rows: expect = .rows(1)
        }
      })
  }

  var body: some View {
    Picker(selection: mode) {
      Text("Nothing").tag(Mode.nothing)
      Text("A first value").tag(Mode.value)
      Text("A number of rows").tag(Mode.rows)
    } label: {
      Text("Expect")
    }
    switch expect {
    case .value(let value):
      TextField(text: Binding(get: { value }, set: { expect = .value($0) }), prompt: Text("Value")) {
        Text("First Value")
      }
    case .rows(let rows):
      LabeledContent("Rows") {
        TextField(value: Binding(get: { Int(rows) }, set: { expect = .rows(UInt32(clamping: $0)) }), format: .number) {
          Text("Rows")
        }
        .labelsHidden()
        .multilineTextAlignment(.trailing)
        .frame(width: 64)
      }
    case nil:
      EmptyView()
    }
  }
}

/// One condition of an `if`: how the screen looks, or a comparison of values.
struct ConditionEditor: View {
  @Binding var condition: Condition
  /// Where the condition is in the block, such as `elseIf.0.`; empty for the first one.
  let at: String
  let context: FieldContext
  let values: InsertableValues

  private var isScreen: Binding<Bool> {
    Binding(
      get: { condition.isScreen },
      set: {
        condition =
          $0
          ? .screen(Reference(image: "", compare: .similar(threshold: 0.95), area: nil, ignore: [], hide: []))
          : .value(ValueCheck(of: "", is: .equals, to: ""))
      })
  }

  var body: some View {
    Picker(selection: isScreen) {
      Text("The Screen").tag(true)
      Text("A Value").tag(false)
    } label: {
      Text("Check")
    }
    .pickerStyle(.segmented)
    switch condition {
    case .screen(let reference):
      ReferenceEditor(
        reference: Binding(get: { reference }, set: { condition = .screen($0) }), field: at + "reference",
        context: context)
      IssueList(issues: context.issues(at + "reference").filter { $0.param("field") == at + "reference" })
    case .value(let check):
      PlaceholderTextField(
        text: Binding(get: { check.of }, set: { condition = .value(ValueCheck(of: $0, is: check.is, to: check.to)) }),
        values: values, label: Text("Value"), prompt: Text(verbatim: "${var.total}"), monospaced: true)
      Picker(selection: Binding(get: { check.is }, set: { condition = .value(ValueCheck(of: check.of, is: $0, to: check.to)) })) {
        Text("is equal to").tag(ValueTest.equals)
        Text("isn’t equal to").tag(ValueTest.notEquals)
        Text("contains").tag(ValueTest.contains)
        Text("is greater than").tag(ValueTest.greaterThan)
        Text("is less than").tag(ValueTest.lessThan)
      } label: {
        Text("Comparison")
      }
      TextField(
        text: Binding(get: { check.to }, set: { condition = .value(ValueCheck(of: check.of, is: check.is, to: $0)) }),
        prompt: Text("Text or number")
      ) {
        Text("Compare With")
      }
      .font(.body.monospaced())
      .autocorrectionDisabled()
      IssueList(issues: context.issues(at + "value"))
    }
  }
}

extension Condition {
  var isScreen: Bool {
    if case .screen = self { true } else { false }
  }
}
