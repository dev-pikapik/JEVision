import JEVisionCore
import SwiftUI

/// Adds an environment, such as staging, to the project: its name and the values tests read
/// from it. It's saved to the project's environments file, next to the tests.
struct AddEnvironmentSheet: View {
  struct Row: Identifiable {
    let id = UUID()
    var name: String
    var value = ""
  }

  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  @State private var name = ""
  @State private var rows: [Row]
  @State private var failure: String?

  /// `valueNames` start the list of values, such as the ones a test is missing.
  init(valueNames: [String] = []) {
    _rows = State(initialValue: valueNames.isEmpty ? [Row(name: "")] : valueNames.map { Row(name: $0) })
  }

  /// Rows with a name or a value; empty ones are left out.
  private var filled: [Row] {
    rows.filter { !$0.name.trimmed.isEmpty || !$0.value.isEmpty }
  }

  /// Why the environment can't be added yet; `nil` when it can.
  private var problem: String? {
    let name = name.trimmed
    guard !name.isEmpty else { return "" }
    let names = filled.map(\.name.trimmed)
    if let twice = Self.repeated(names) {
      return String(localized: "There are two values named \(twice).")
    }
    return project.environments.problem(adding: name, valueNames: names).map(Self.describe)
  }

  var body: some View {
    let problem = problem
    VStack(alignment: .leading, spacing: 0) {
      VStack(alignment: .leading, spacing: 4) {
        Text("Add Environment")
          .font(.title3.weight(.semibold))
        Text("An environment is a set of values tests read, such as the address of a test server. Each environment gets its own values; you pick the one runs use at the foot of the sidebar.")
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
      .padding([.horizontal, .top], 20)

      Form {
        Section {
          TextField(text: $name, prompt: Text(verbatim: "staging")) { Text("Name") }
        } footer: {
          if let problem, !problem.isEmpty {
            Text(problem).foregroundStyle(.red)
          }
        }
        Section {
          ForEach($rows) { $row in
            HStack {
              TextField(text: $row.name, prompt: Text(verbatim: "BASE_URL")) { Text("Value Name") }
                .labelsHidden()
                .monospaced()
              TextField(text: $row.value, prompt: Text(verbatim: "https://staging.example.com")) { Text("Value") }
                .labelsHidden()
              Button("Remove", systemImage: "minus.circle") { rows.removeAll { $0.id == row.id } }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .help(Text("Remove this value"))
            }
          }
          Button("Add Value", systemImage: "plus") { rows.append(Row(name: "")) }
            .buttonStyle(.borderless)
        } header: {
          Text("Values")
        } footer: {
          Text("A test reads a value as ${env.NAME}. The file \(environmentsFileName()) in the project folder keeps them and goes to git with the tests, so passwords don’t belong here: set those as secrets.")
            .foregroundStyle(.secondary)
        }
      }
      .formStyle(.grouped)

      if let failure {
        Text(failure)
          .foregroundStyle(.red)
          .font(.callout)
          .padding(.horizontal, 20)
      }
      HStack {
        Spacer()
        Button("Cancel", role: .cancel) { dismiss() }
          .keyboardShortcut(.cancelAction)
        Button("Add", action: add)
          .keyboardShortcut(.defaultAction)
          .disabled(problem != nil)
      }
      .padding(20)
    }
    .frame(width: 520)
    .frame(minHeight: 400)
  }

  private func add() {
    let values = Dictionary(filled.map { ($0.name.trimmed, $0.value) }, uniquingKeysWith: { $1 })
    do {
      try project.environments.add(name.trimmed, values: values)
      dismiss()
    } catch let error as NewEnvironmentError {
      failure = Self.describe(error)
    } catch {
      failure = error.localizedDescription
    }
  }

  /// The first name in `names` that comes twice.
  static func repeated(_ names: [String]) -> String? {
    var seen = Set<String>()
    return names.first { !seen.insert($0).inserted }
  }

  static func describe(_ error: NewEnvironmentError) -> String {
    switch error {
    case .InvalidName:
      String(localized: "Use up to 64 Latin letters, digits, - and _ in the name, without spaces.")
    case .Taken(let name):
      String(localized: "There’s an environment \(name) already.")
    case .InvalidValueName(let name) where name.isEmpty:
      String(localized: "Every value needs a name.")
    case .InvalidValueName(let name):
      String(localized: "“\(name)” can’t be a value name: use Latin letters, digits and _, not starting with a digit.")
    case .Unreadable(let file, let reason):
      String(localized: "\(file) can’t be read: \(reason)")
    case .Unwritable(let file, let reason):
      String(localized: "\(file) can’t be saved: \(reason)")
    }
  }
}

extension String {
  fileprivate var trimmed: String { trimmingCharacters(in: .whitespaces) }
}
