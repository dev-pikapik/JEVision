import JEVisionCore
import SwiftUI

/// The test as a whole: its name, app, data, secrets and retries.
struct TestDetailsView: View {
  @Environment(ProjectStore.self) private var project
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
          LabeledContent {
            Stepper(value: binding(Int(flow.retries), "retries") { $0.retries = UInt32(clamping: $1) }, in: 0...10) {
              Text(flow.retries, format: .number).monospacedDigit()
            }
          } label: {
            Text("Retries")
            Text("How many more times to run the test if it fails.")
          }
        }
        let files = DataColumns.files(in: flow)
        if !files.isEmpty {
          Section("Data Files") {
            ForEach(files, id: \.self) { Label($0, systemImage: "tablecells") }
          }
        }
        let secrets = requiredSecrets(flow: flow)
        Section {
          if secrets.isEmpty {
            Text("This test doesn’t use secrets. Add one in a Type Text block with Insert.")
              .foregroundStyle(.secondary)
          }
          ForEach(secrets, id: \.self) { name in
            let isSet = secretsVersion >= 0 && SecretStore.isSet(name)
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
          Text("Secrets")
        } footer: {
          Text("Values are kept in the Keychain on this Mac or read from environment variables of the same name.")
            .foregroundStyle(.secondary)
        }
        IssueList(issues: issues.filter { !["name", "app"].contains($0.param("field") ?? "") })
      }
      .formStyle(.grouped)
      .frame(width: 400)
      .frame(minHeight: 300, maxHeight: 560)
      .sheet(item: $settingSecret) { secret in
        SetSecretSheet(name: secret.id) { secretsVersion += 1 }
      }
    }
  }

  private func binding<T>(_ value: T, _ field: String, _ set: @escaping (inout Flow, T) -> Void) -> Binding<T> {
    let document = document
    return Binding(get: { value }, set: { new in document.updateFlow(field: field) { set(&$0, new) } })
  }
}

private struct SecretName: Identifiable {
  let id: String
}

/// Asks for a secret's value and puts it into the Keychain. The value is never shown.
private struct SetSecretSheet: View {
  let name: String
  let saved: () -> Void
  @Environment(\.dismiss) private var dismiss
  @State private var value = ""
  @State private var error: String?

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      Text("Set \(name)")
        .font(.headline)
      Text("The value is kept in the Keychain on this Mac. JEVision types it during runs and never shows it.")
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
            try SecretStore.save(value, for: name)
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
