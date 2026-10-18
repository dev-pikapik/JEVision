import SwiftUI

/// What recording needs before it can start, each with a way to fix it.
struct RecordReadinessSheet: View {
  @Environment(RecordCenter.self) private var records
  @Environment(PermissionsMonitor.self) private var monitor
  let request: RecordCenter.Request
  @State private var app: String?
  private let catalog = AppCatalog.shared

  init(request: RecordCenter.Request) {
    self.request = request
    _app = State(initialValue: RecordCenter.app(of: request))
  }

  var body: some View {
    let _ = monitor.granted
    let missing = RecordCenter.missingPermissions()
    let installed = app.map(catalog.isInstalled) ?? false
    VStack(alignment: .leading, spacing: 0) {
      VStack(alignment: .leading, spacing: 4) {
        Text("Before you record")
          .font(.title3.weight(.semibold))
        Text("JEVision watches what you do in the app and turns it into steps of the test.")
          .foregroundStyle(.secondary)
      }
      .padding([.horizontal, .top], 20)

      Form {
        if RecordCenter.app(of: request).map(catalog.isInstalled) != true {
          Section {
            LabeledContent("App") {
              AppPicker(bundleID: $app, noneTitle: Text("Choose App"))
            }
          } header: {
            Text("App")
          } footer: {
            Text("The app you’ll record in. It becomes the test’s app.")
              .foregroundStyle(.secondary)
          }
        }
        if !missing.isEmpty {
          Section("Permissions") {
            ForEach(Permission.allCases) { PermissionRow(permission: $0) }
          }
        }
      }
      .formStyle(.grouped)

      HStack {
        Spacer()
        Button("Cancel", role: .cancel) { records.blocked = nil }
          .keyboardShortcut(.cancelAction)
        Button("Record") {
          if let app { records.confirm(app: app) }
        }
        .keyboardShortcut(.defaultAction)
        .disabled(!missing.isEmpty || !installed)
      }
      .padding([.horizontal, .bottom], 20)
    }
    .frame(width: 520)
    .frame(minHeight: 240)
    .task { await monitor.watch() }
    .onAppear { catalog.load() }
  }
}
