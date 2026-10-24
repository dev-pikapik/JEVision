import JEVisionCore
import SwiftUI

/// What a test needs before it can run, each with a way to fix it.
struct RunReadinessSheet: View {
  @Environment(RunCenter.self) private var runs
  @Environment(PermissionsMonitor.self) private var monitor
  @Environment(ProjectStore.self) private var project
  @Environment(\.undoManager) private var undoManager
  let request: RunCenter.Request
  @State private var settingSecret: SecretName?
  @State private var addingEnvironment = false
  /// Bumped after a secret or an environment is saved, to check again.
  @State private var secretsVersion = 0

  var body: some View {
    let _ = (secretsVersion, monitor.granted)
    let missing = RunCenter.missing(for: request)
    let values = missing.values
    let ready = missing.permissions.isEmpty && values.isEmpty && missing.errors == 0
    VStack(alignment: .leading, spacing: 0) {
      VStack(alignment: .leading, spacing: 4) {
        Group {
          // Every test a batch has yet to start waits for this one.
          if let batch = runs.batch, batch.total - batch.done - batch.running > 1 {
            Text("Before \(batch.total - batch.done - batch.running) tests can run")
          } else {
            Text("Before the test can run")
          }
        }
        .font(.title3.weight(.semibold))
        Text("JEVision needs a few things to drive this Mac for you.")
          .foregroundStyle(.secondary)
      }
      .padding([.horizontal, .top], 20)

      Form {
        if !missing.permissions.isEmpty {
          Section("Permissions") {
            ForEach(Permission.allCases) { PermissionRow(permission: $0) }
          }
        }
        if let problem = values.problem {
          Section("Environment") {
            Label {
              Text(problem)
            } icon: {
              Image(systemName: "exclamationmark.octagon.fill").foregroundStyle(.red)
            }
            Button("Edit Environments…") { project.environments.edit() }
          }
        } else if !values.values.isEmpty {
          Section {
            ForEach(values.values, id: \.self) { name in
              Label {
                Text(verbatim: name)
              } icon: {
                Image(systemName: "server.rack").foregroundStyle(.orange)
              }
            }
            if values.environment == nil {
              Button("Add Environment…") { addingEnvironment = true }
            } else {
              Button("Edit Environments…") { project.environments.edit() }
            }
          } header: {
            Text("Environment Values")
          } footer: {
            Group {
              if let environment = values.environment {
                Text("The environment \(environment) has no value for these names. Add them to \(environmentsFileName()).")
              } else {
                Text("The test reads these values from an environment, and the project has none yet.")
              }
            }
            .foregroundStyle(.secondary)
          }
        }
        if !values.secrets.isEmpty {
          Section {
            ForEach(values.secrets, id: \.self) { name in
              LabeledContent {
                Button("Set…") { settingSecret = SecretName(id: name) }
                  .accessibilityLabel(Text("Set \(name)"))
              } label: {
                Label {
                  Text(verbatim: name)
                } icon: {
                  Image(systemName: "key.fill").foregroundStyle(.orange)
                }
              }
            }
          } header: {
            Text("Secrets")
          } footer: {
            Group {
              if let environment = values.environment {
                Text("The test types these values in \(environment). They’re kept in the Keychain and never shown.")
              } else {
                Text("The test types these values. They’re kept in the Keychain and never shown.")
              }
            }
            .foregroundStyle(.secondary)
          }
        }
        if missing.errors > 0 {
          Section {
            Label {
              Text("The test has problems. They’re marked on the blocks; fix them and run again.")
            } icon: {
              Image(systemName: "exclamationmark.octagon.fill").foregroundStyle(.red)
            }
          }
        }
      }
      .formStyle(.grouped)

      HStack {
        Spacer()
        Button("Cancel", role: .cancel) { runs.blocked = nil }
          .keyboardShortcut(.cancelAction)
        Button("Run") { runs.confirm() }
          .keyboardShortcut(.defaultAction)
          .disabled(!ready)
      }
      .padding([.horizontal, .bottom], 20)
    }
    .frame(width: 520)
    .frame(minHeight: 280)
    .task { await monitor.watch() }
    .sheet(item: $settingSecret) { secret in
      ProjectSecretSheet(name: secret.id, project: request.project, environment: missing.values.environment) { secretsVersion += 1 }
    }
    .sheet(isPresented: $addingEnvironment, onDismiss: { secretsVersion += 1 }) {
      AddEnvironmentSheet(valueNames: values.values, undoManager: undoManager)
    }
  }
}
