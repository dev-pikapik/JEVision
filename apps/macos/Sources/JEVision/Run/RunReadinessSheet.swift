import JEVisionCore
import SwiftUI

/// What a test needs before it can run, each with a way to fix it.
struct RunReadinessSheet: View {
  @Environment(RunCenter.self) private var runs
  @Environment(PermissionsMonitor.self) private var monitor
  let request: RunCenter.Request
  @State private var settingSecret: SecretName?
  /// Bumped after a secret is saved, to check again.
  @State private var secretsVersion = 0

  var body: some View {
    let _ = (secretsVersion, monitor.granted)
    let missing = RunCenter.missing(for: request)
    let ready = missing.permissions.isEmpty && missing.secrets.isEmpty && missing.errors == 0
    VStack(alignment: .leading, spacing: 0) {
      VStack(alignment: .leading, spacing: 4) {
        Text("Before the test can run")
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
        if !missing.secrets.isEmpty {
          Section {
            ForEach(missing.secrets, id: \.self) { name in
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
            Text("The test types these values. They’re kept in the Keychain and never shown.")
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
      SetSecretSheet(name: secret.id) { secretsVersion += 1 }
    }
  }
}
