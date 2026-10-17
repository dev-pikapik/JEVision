import SwiftUI

struct PermissionsStep: View {
  @Environment(PermissionsMonitor.self) private var monitor

  var body: some View {
    VStack(spacing: 0) {
      PageHeader(
        symbol: monitor.allGranted ? "checkmark" : "hand.raised.fill", color: .blue,
        title: monitor.allGranted ? Text("All set") : Text("Two permissions to see and click"),
        message: Text("macOS asks for these for any app that controls the screen. JEVision uses them only while a test runs or records."),
        tileSize: 64)
      Form {
        PermissionsSection()
      }
      .formStyle(.grouped)
      .scrollContentBackground(.hidden)
    }
    .padding(.horizontal, 32)
    .padding(.top, 28)
  }
}
