import JEVisionCore
import SwiftUI

/// Which element to find by what the app tells macOS accessibility: its kind, its title, its id
/// and, when several fit, which one of them.
struct ElementQueryEditor: View {
  @Binding var query: ElementQuery

  private var kinds: [ElementKind] { elementKinds() }

  private var hasIndex: Binding<Bool> {
    Binding(get: { query.index != nil }, set: { query.index = $0 ? 0 : nil })
  }

  private var index: Binding<Int> {
    Binding(get: { Int(query.index ?? 0) }, set: { query.index = UInt32(clamping: max($0, 0)) })
  }

  var body: some View {
    Picker(selection: $query.role) {
      Text("Any Kind").tag("")
      ForEach(kinds, id: \.name) { kind in
        Text(verbatim: kind.label.prefix(1).uppercased() + kind.label.dropFirst()).tag(kind.name)
      }
      // A kind this version doesn't know stays as written; the problem under the field says so.
      if !query.role.isEmpty, !kinds.contains(where: { $0.name == query.role }) {
        Text(verbatim: query.role).tag(query.role)
      }
    } label: {
      Text("Kind")
    }
    TextField(text: $query.title, prompt: Text("Save")) { Text("Title") }
      .autocorrectionDisabled()
    TextField(text: $query.id, prompt: Text(verbatim: "save-button")) { Text("Identifier") }
      .font(.body.monospaced())
      .autocorrectionDisabled()
    Toggle("Pick One of Several", isOn: hasIndex)
    if query.index != nil {
      LabeledContent("Index") {
        HStack {
          TextField(value: index, format: .number.grouping(.never)) { Text("Index") }
            .labelsHidden()
            .multilineTextAlignment(.trailing)
            .frame(width: 64)
          Stepper(value: index, in: 0...9_999) { Text("Index") }
            .labelsHidden()
        }
      }
    }
    Text("Fill in at least one of the kind, the title and the identifier. The title is the element’s label, exactly as accessibility reads it; it can hold ${var.NAME}. When several elements fit, the index picks one: 0 is the first, in the order the window lists them, usually from the top.")
      .font(.callout)
      .foregroundStyle(.secondary)
      .fixedSize(horizontal: false, vertical: true)
  }
}

/// An element and the state it should be in, for a check, a condition or a wait.
struct ElementCheckEditor: View {
  @Binding var check: ElementCheck
  /// Where the check is in the block, such as `element` or `until.element`.
  let field: String
  let context: FieldContext

  private var choice: Binding<ElementState.Choice> {
    Binding(get: { check.is.choice }, set: { check.is = ElementState($0, text: check.is.text ?? "") })
  }

  private var text: Binding<String> {
    Binding(get: { check.is.text ?? "" }, set: { check.is = ElementState(check.is.choice, text: $0) })
  }

  var body: some View {
    ElementQueryEditor(query: $check.element)
    Picker(selection: choice) {
      ForEach(ElementState.Choice.allCases, id: \.self) { choice in
        Text(choice.title).tag(choice)
      }
    } label: {
      Text("State")
    }
    if check.is.text != nil {
      TextField(text: text, prompt: Text("Text")) { Text("Text") }
        .autocorrectionDisabled()
    }
    IssueList(issues: context.issues(field))
  }
}

extension ElementState {
  /// The states an editor offers, in the order of the file format.
  enum Choice: CaseIterable {
    case present, absent, enabled, disabled, selected, notSelected
    case valueEquals, valueContains, titleEquals, titleContains

    var title: LocalizedStringKey {
      switch self {
      case .present: "Is in the window"
      case .absent: "Isn’t in the window"
      case .enabled: "Is enabled"
      case .disabled: "Is disabled"
      case .selected: "Is selected or checked"
      case .notSelected: "Isn’t selected or checked"
      case .valueEquals: "Value is"
      case .valueContains: "Value contains"
      case .titleEquals: "Title is"
      case .titleContains: "Title contains"
      }
    }
  }

  /// The state `choice` names; `text` goes to the ones that compare text.
  init(_ choice: Choice, text: String) {
    switch choice {
    case .present: self = .present
    case .absent: self = .absent
    case .enabled: self = .enabled
    case .disabled: self = .disabled
    case .selected: self = .selected
    case .notSelected: self = .notSelected
    case .valueEquals: self = .valueEquals(text)
    case .valueContains: self = .valueContains(text)
    case .titleEquals: self = .titleEquals(text)
    case .titleContains: self = .titleContains(text)
    }
  }

  var choice: Choice {
    switch self {
    case .present: .present
    case .absent: .absent
    case .enabled: .enabled
    case .disabled: .disabled
    case .selected: .selected
    case .notSelected: .notSelected
    case .valueEquals: .valueEquals
    case .valueContains: .valueContains
    case .titleEquals: .titleEquals
    case .titleContains: .titleContains
    }
  }

  /// The text the state compares with; nil for the ones that compare none.
  var text: String? {
    switch self {
    case .valueEquals(let t), .valueContains(let t), .titleEquals(let t), .titleContains(let t): t
    default: nil
    }
  }
}

extension ElementCheck {
  /// What a new check looks for: an element to fill in, there in the window.
  static let new = ElementCheck(element: ElementQuery(id: "", role: "", title: "", index: nil), is: .present)
}

/// Command-line arguments an app gets when a block starts it, one per row.
struct ArgumentsEditor: View {
  @Binding var arguments: [String]
  let context: FieldContext

  var body: some View {
    ForEach(arguments.indices, id: \.self) { index in
      HStack {
        TextField(
          text: Binding(get: { arguments[index] }, set: { arguments[index] = $0 }),
          prompt: Text(verbatim: "--demo-mode")
        ) {
          Text("Argument \(index + 1)")
        }
        .labelsHidden()
        .font(.body.monospaced())
        .autocorrectionDisabled()
        .accessibilityLabel(Text("Argument \(index + 1)"))
        Button("Remove", systemImage: "minus.circle") { arguments.remove(at: index) }
          .labelStyle(.iconOnly)
          .buttonStyle(.borderless)
      }
      IssueList(issues: context.issues("arguments.\(index)"))
    }
    Button("Add Argument", systemImage: "plus") { arguments.append("") }
  }
}

/// Environment variables an app gets when a block starts it. Rows keep the order they are typed
/// in, though the file keeps a name only once: a row without a name, or with a name used above,
/// is left out of it.
struct EnvironmentEditor: View {
  @Binding var environment: [String: String]
  let context: FieldContext
  @State private var rows: [NamedValue] = []

  var body: some View {
    ForEach(rows.indices, id: \.self) { index in
      HStack {
        TextField(text: row(index, \.name), prompt: Text(verbatim: "API_URL")) { Text("Name") }
          .labelsHidden()
          .font(.body.monospaced())
          .autocorrectionDisabled()
          .accessibilityLabel(Text("Name \(index + 1)"))
        TextField(text: row(index, \.value), prompt: Text(verbatim: "${env.API_URL}")) { Text("Value") }
          .labelsHidden()
          .font(.body.monospaced())
          .autocorrectionDisabled()
          .accessibilityLabel(Text("Value \(index + 1)"))
        Button("Remove", systemImage: "minus.circle") {
          rows.remove(at: index)
          environment = Self.environment(of: rows)
        }
        .labelStyle(.iconOnly)
        .buttonStyle(.borderless)
      }
      let name = rows[index].name
      if rows[..<index].contains(where: { $0.name == name && !name.isEmpty }) {
        Text("This name is already used above, so this row is left out.")
          .font(.callout)
          .foregroundStyle(.orange)
      } else if !name.isEmpty {
        IssueList(issues: context.issues("environment.\(name)"))
      }
    }
    Button("Add Variable", systemImage: "plus") { rows.append(NamedValue(name: "", value: "")) }
      .onAppear { rows = Self.rows(of: environment) }
      .onChange(of: environment) { _, new in
        if Self.environment(of: rows) != new { rows = Self.rows(of: new) }
      }
  }

  private func row(_ index: Int, _ path: WritableKeyPath<NamedValue, String>) -> Binding<String> {
    Binding(
      get: { rows.indices.contains(index) ? rows[index][keyPath: path] : "" },
      set: { value in
        guard rows.indices.contains(index) else { return }
        rows[index][keyPath: path] = value
        environment = Self.environment(of: rows)
      })
  }

  /// The variables of a block as rows, by name.
  nonisolated static func rows(of environment: [String: String]) -> [NamedValue] {
    environment.sorted { $0.key < $1.key }.map { NamedValue(name: $0.key, value: $0.value) }
  }

  /// The variables the rows give: each name once, from its first row; rows without a name left out.
  nonisolated static func environment(of rows: [NamedValue]) -> [String: String] {
    Dictionary(rows.filter { !$0.name.isEmpty }.map { ($0.name, $0.value) }, uniquingKeysWith: { first, _ in first })
  }
}
