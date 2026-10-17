import SwiftUI

struct SettingsView: View {
  @Environment(PermissionsMonitor.self) private var monitor

  var body: some View {
    TabView {
      Tab("General", systemImage: "gearshape") {
        GeneralSettingsView()
      }
      // The warning sign stays on the tab until every permission is on.
      Tab("Permissions", systemImage: monitor.allGranted ? "hand.raised" : "exclamationmark.triangle") {
        PermissionsSettingsView()
      }
    }
    .frame(width: 600, height: 540)
    .task { await monitor.watch() }
  }
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
