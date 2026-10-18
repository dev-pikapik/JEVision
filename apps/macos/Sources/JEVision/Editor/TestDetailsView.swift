import JEVisionCore
import JEVisionHands
import SwiftUI

/// The test as a whole: its name, app, data, secrets, retries and the tests it runs after.
struct TestDetailsView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  let document: FlowDocument
  @State private var settingSecret: SecretName?
  /// Bumped after a secret is saved, to read the statuses again.
  @State private var secretsVersion = 0

  var body: some View {
    if let flow = document.flow {
      let issues = document.issues.filter { $0.param("block") == nil }
      Form {
        Section {
          TextField(text: binding(flow.name, "name") { $0.name = $1 }) { Text("Name") }
          IssueList(issues: issues.filter { $0.param("field") == "name" })
          TextField(
            text: binding(flow.description ?? "", "description") { $0.description = $1.isEmpty ? nil : $1 },
            prompt: Text("What the test checks"), axis: .vertical
          ) { Text("Description") }
          .lineLimit(1...4)
          LabeledContent("App") {
            AppPicker(
              bundleID: binding(flow.app.isEmpty ? nil : flow.app, "app") { $0.app = $1 ?? "" },
              noneTitle: Text("Choose App"))
          }
          IssueList(issues: issues.filter { $0.param("field") == "app" })
        }
        Section {
          TagsField(tags: flow.tags) { tags in document.updateFlow(field: "tags") { $0.tags = tags } }
          IssueList(issues: issues.filter { $0.param("field") == "tags" || $0.code == "flow.tags.duplicate" })
          Picker(selection: binding(flow.testType, "testType") { $0.testType = $1 }) {
            Text("Not Set").tag(TestType?.none)
            Divider()
            ForEach([TestType.smoke, .regression, .acceptance], id: \.self) { Text(TestFields.title($0)).tag(TestType?.some($0)) }
          } label: {
            Text("Type")
          }
          Picker(selection: binding(flow.priority, "priority") { $0.priority = $1 }) {
            Text("Not Set").tag(Priority?.none)
            Divider()
            ForEach([Priority.critical, .high, .medium, .low], id: \.self) { Text(TestFields.title($0)).tag(Priority?.some($0)) }
          } label: {
            Text("Priority")
          }
          Picker(selection: binding(flow.status, "status") { $0.status = $1 }) {
            Text("Not Set").tag(TestStatus?.none)
            Divider()
            ForEach([TestStatus.draft, .ready, .outdated], id: \.self) { Text(TestFields.title($0)).tag(TestStatus?.some($0)) }
          } label: {
            Text("Status")
          }
        }
        Section {
          Picker(selection: binding(flow.retries, "retries") { $0.retries = $1 }) {
            Text("Project Default (\(project.settings.retries ?? 0))").tag(UInt32?.none)
            Divider()
            ForEach(UInt32(0)...10, id: \.self) { Text($0, format: .number).tag(UInt32?.some($0)) }
          } label: {
            Text("Retries")
            Text("How many more times to run the test if it fails. The project default is set in Settings › Advanced.")
          }
        }
        Section {
          DependenciesField(document: document, dependsOn: flow.dependsOn)
          IssueList(issues: issues.filter { $0.param("field") == "dependsOn" })
        }
        if let file = project.testID(document.url) {
          QuarantineSection(file: file, name: flow.name)
        }
        let files = DataColumns.files(in: flow)
        if !files.isEmpty {
          Section("Data Files") {
            ForEach(files, id: \.self) { Label($0, systemImage: "tablecells") }
          }
        }
        let secrets = requiredSecrets(flow: flow)
        let environment = project.environments.current.selected?.name
        Section {
          if secrets.isEmpty {
            Text("This test doesn’t use secrets. Add one in a Type Text block with Insert.")
              .foregroundStyle(.secondary)
          }
          ForEach(secrets, id: \.self) { name in
            let isSet = secretsVersion >= 0 && SecretStore.isSet(name, environment: environment)
            LabeledContent {
              HStack {
                Label {
                  Text(isSet ? "Set" : "Not set")
                } icon: {
                  Image(systemName: isSet ? "checkmark.circle.fill" : "circle.dashed")
                    .foregroundStyle(isSet ? .green : .secondary)
                }
                Button("Set…") { settingSecret = SecretName(id: name) }
                  .accessibilityLabel(Text("Set \(name)"))
              }
            } label: {
              Text(verbatim: name)
            }
          }
        } header: {
          if let environment {
            Text("Secrets in \(environment)")
          } else {
            Text("Secrets")
          }
        } footer: {
          Text("Values are kept in the Keychain on this Mac or read from environment variables of the same name.")
            .foregroundStyle(.secondary)
        }
        if project.git.status?.hasCommits == true, let path = project.selection {
          Section {
            LabeledContent {
              Button("History…") {
                dismiss()
                project.historyPath = path
              }
            } label: {
              Text("History")
              Text("Every committed version of this test.")
            }
          }
        }
        IssueList(issues: issues.filter {
          !["name", "app", "tags", "dependsOn"].contains($0.param("field") ?? "") && $0.code != "flow.tags.duplicate"
        })
      }
      .formStyle(.grouped)
      .frame(width: 400)
      .frame(minHeight: 300, maxHeight: 560)
      .sheet(item: $settingSecret) { secret in
        SetSecretSheet(name: secret.id, environment: project.environments.current.selected?.name) {
          secretsVersion += 1
        }
      }
    }
  }

  private func binding<T>(_ value: T, _ field: String, _ set: @escaping (inout Flow, T) -> Void) -> Binding<T> {
    let document = document
    return Binding(get: { value }, set: { new in document.updateFlow(field: field) { set(&$0, new) } })
  }
}

/// Tags typed as one line, separated by commas; saved when the field is left or Return is pressed.
private struct TagsField: View {
  let tags: [String]
  let save: ([String]) -> Void
  @State private var text = ""
  @FocusState private var focused: Bool

  var body: some View {
    TextField(text: $text, prompt: Text(verbatim: "smoke, sign-in")) { Text("Tags") }
      .focused($focused)
      .onSubmit(commit)
      .onChange(of: focused) { if !focused { commit() } }
      .onDisappear(perform: commit)
      .onChange(of: tags, initial: true) { if !focused { text = tags.joined(separator: ", ") } }
  }

  private func commit() {
    let new = TestFields.tags(text)
    if new != tags { save(new) }
    text = new.joined(separator: ", ")
  }
}

enum TestFields {
  /// Tags from "a, b,, a": trimmed, without empty ones and repeats, in the order typed.
  static func tags(_ text: String) -> [String] {
    var seen = Set<String>()
    return text.split(separator: ",")
      .map { $0.trimmingCharacters(in: .whitespaces) }
      .filter { !$0.isEmpty && seen.insert($0).inserted }
  }

  static func title(_ type: TestType) -> String {
    switch type {
    case .smoke: String(localized: "Smoke")
    case .regression: String(localized: "Regression")
    case .acceptance: String(localized: "Acceptance")
    }
  }

  static func title(_ priority: Priority) -> String {
    switch priority {
    case .critical: String(localized: "Critical")
    case .high: String(localized: "High")
    case .medium: String(localized: "Medium")
    case .low: String(localized: "Low")
    }
  }

  static func title(_ status: TestStatus) -> String {
    switch status {
    case .draft: String(localized: "Draft")
    case .ready: String(localized: "Ready")
    case .outdated: String(localized: "Outdated")
    }
  }
}

struct SecretName: Identifiable {
  let id: String
}

/// Asks for a secret's value and puts it into the Keychain. The value is never shown.
struct SetSecretSheet: View {
  let name: String
  var title: Text?
  var message: Text?
  var service = SecretStore.tests
  let saved: () -> Void
  @Environment(\.dismiss) private var dismiss
  @State private var value = ""
  @State private var error: String?

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      (title ?? Text("Set \(name)"))
        .font(.headline)
      (message ?? Text("The value is kept in the Keychain on this Mac. JEVision types it during runs and never shows it."))
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      SecureField(text: $value, prompt: Text("Value")) { Text("Value") }
      if let error {
        Text(error).foregroundStyle(.red).font(.callout)
      }
      HStack {
        Spacer()
        Button("Cancel", role: .cancel) { dismiss() }
          .keyboardShortcut(.cancelAction)
        Button("Save") {
          do {
            try SecretStore.save(value, for: name, in: service)
            value = ""
            saved()
            dismiss()
          } catch {
            self.error = error.localizedDescription
          }
        }
        .keyboardShortcut(.defaultAction)
        .disabled(value.isEmpty)
      }
    }
    .padding(20)
    .frame(width: 360)
  }
}

extension SetSecretSheet {
  /// Asks for the secret's value in `environment`, or for every environment when there's none.
  init(name: String, environment: String?, saved: @escaping () -> Void) {
    self.init(
      name: name,
      message: environment.map {
        Text("The value is kept in the Keychain on this Mac for \($0) runs. JEVision types it during runs and never shows it.")
      },
      service: SecretStore.service(for: environment), saved: saved)
  }
}
