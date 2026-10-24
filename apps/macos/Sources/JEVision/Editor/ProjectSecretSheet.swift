import JEVisionHands
import SwiftUI

/// The project and shared choices are explicit. Existing shared values are never imported
/// merely by opening a project, and copying one leaves the original available to its users.
struct ProjectSecretSheet: View {
  let name: String
  let project: URL?
  let environment: String?
  let saved: () -> Void
  @Environment(\.dismiss) private var dismiss
  @State private var value = ""
  @State private var shared = false
  @State private var failure: String?

  var body: some View {
    VStack(alignment: .leading, spacing: 14) {
      Text("Set \(name)").font(.headline)
      Text("Secrets belong to this project. Shared values are used only when you choose them.")
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      if let project {
        let scope = ProjectSecrets(project: project)
        LabeledContent("Project", value: project.lastPathComponent)
        if let environment { LabeledContent("Environment", value: environment) }
        if scope.usesShared(name, environment: environment) {
          Label("Using a shared value", systemImage: "person.2")
          Button("Stop Using Shared Value") {
            perform { try scope.setUsesShared(false, for: name, environment: environment) }
          }
        }
        if scope.hasShared(name, environment: environment) {
          HStack {
            Button("Use Shared Value") {
              perform { try scope.setUsesShared(true, for: name, environment: environment) }
            }
            Button("Copy to This Project") {
              perform {
                guard try scope.copyShared(name, environment: environment) else {
                  throw CocoaError(.fileNoSuchFile)
                }
              }
            }
          }
        }
        SecureField("Value", text: $value)
        Toggle("Share this value on this Mac", isOn: $shared)
        Group {
          if shared {
            Text("Other projects can use this value only after you enable shared access there.")
          } else {
            Text("Only this project can use this value. Existing shared values stay unchanged.")
          }
        }.font(.callout).foregroundStyle(.secondary)
      }
      if let failure { Text(failure).foregroundStyle(.red).font(.callout) }
      HStack {
        Spacer()
        Button("Cancel", role: .cancel) { value = ""; dismiss() }.keyboardShortcut(.cancelAction)
        Button("Save") {
          guard let project else { return }
          let scope = ProjectSecrets(project: project)
          perform {
            if shared {
              try SecretStore.save(value, for: name, in: SecretStore.service(for: environment))
              try scope.setUsesShared(true, for: name, environment: environment)
            } else {
              try scope.save(value, for: name, environment: environment)
            }
          }
        }
        .keyboardShortcut(.defaultAction)
        .disabled(value.isEmpty || project == nil)
      }
    }
    .padding(20)
    .frame(width: 440)
  }

  private func perform(_ change: () throws -> Void) {
    do {
      try change()
      value = ""
      saved()
      dismiss()
    } catch { failure = error.localizedDescription }
  }
}
