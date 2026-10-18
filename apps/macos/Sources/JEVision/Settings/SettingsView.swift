import SwiftUI

struct SettingsView: View {
  @Environment(PermissionsMonitor.self) private var monitor
  @AppStorage(SettingsTab.key) private var tab = SettingsTab.general

  var body: some View {
    TabView(selection: $tab) {
      Tab("General", systemImage: "gearshape", value: .general) {
        GeneralSettingsView()
      }
      Tab("AI", systemImage: "sparkles", value: .ai) {
        AISettingsView()
      }
      // The warning sign stays on the tab until every permission is on.
      Tab("Permissions", systemImage: monitor.allGranted ? "hand.raised" : "exclamationmark.triangle", value: .permissions) {
        PermissionsSettingsView()
      }
      Tab("iPhone & Remote", systemImage: "iphone", value: .remote) {
        RemoteSettingsView()
      }
      Tab("Git and GitHub", systemImage: "arrow.triangle.branch", value: .git) {
        GitSettingsView()
      }
      Tab("Notifications", systemImage: "bell", value: .notifications) {
        NotificationSettingsView()
      }
      Tab("Advanced", systemImage: "slider.horizontal.3", value: .advanced) {
        AdvancedSettingsView()
      }
    }
    .frame(width: 600, height: 540)
    .task { await monitor.watch() }
  }
}

/// The open Settings tab; set it before opening Settings to show a given one.
enum SettingsTab: String {
  case general, ai, permissions, remote, git, notifications, advanced

  static let key = "settingsTab"
}

private struct PermissionsSettingsView: View {
  @Environment(PermissionsMonitor.self) private var monitor

  var body: some View {
    Form {
      Section {
        PageHeader(
          symbol: monitor.allGranted ? "checkmark" : "hand.raised.fill", color: .blue,
          title: monitor.allGranted ? Text("All set") : Text("JEVision needs two permissions"),
          message: monitor.allGranted
            ? Text("JEVision can see other apps and click in them.")
            : Text("Without them, macOS won’t let JEVision see other apps or click in them."))
      }
      PermissionsSection()
    }
    .formStyle(.grouped)
  }
}
