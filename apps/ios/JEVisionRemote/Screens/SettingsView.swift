import SwiftUI

struct SettingsView: View {
  @Environment(AppModel.self) private var model
  @Environment(\.dismiss) private var dismiss
  /// Closes Settings and opens the camera.
  let pairMac: () -> Void
  @State private var forgetting: HostRecord?
  private let version = AppVersion()

  var body: some View {
    @Bindable var lock = model.lock
    NavigationStack {
      Form {
        Section {
          ForEach(model.hosts) { host in
            HStack {
              Label(host.name, systemImage: "desktopcomputer")
              Spacer()
              Button("Forget", role: .destructive) { forgetting = host }
                .buttonStyle(.borderless)
            }
          }
          Button("Pair a Mac", systemImage: "qrcode.viewfinder", action: pairMac)
        } header: {
          Text("Paired Macs")
        } footer: {
          Text("To stop a Mac from trusting this iPhone, remove the iPhone in JEVision on that Mac too.")
        }

        Section {
          Picker("Lock After", selection: $lock.timeout) {
            ForEach(LockTimeout.allCases) { timeout in
              Text(timeout.title).tag(timeout)
            }
          }
          Button("Lock Now", systemImage: "lock") {
            dismiss()
            model.lock.lock()
          }
        } header: {
          Text("App Lock")
        } footer: {
          Text("JEVision asks for Face ID or your passcode when you open it, after it has been in the background this long, and at least every 8 hours.")
        }

        Section {
          LabeledContent("Version", value: "\(version.version) (\(version.build))")
          ForEach(AboutLink.allCases) { link in
            Link(destination: link.url) {
              Label(link.title, systemImage: link.symbol)
            }
          }
          NavigationLink {
            LicenseView()
          } label: {
            Label("License", systemImage: "doc.text")
          }
          .accessibilityIdentifier("license")
          NavigationLink {
            ThirdPartyLicensesView()
          } label: {
            Label("Third-Party Licenses", systemImage: "books.vertical")
          }
          .accessibilityIdentifier("thirdPartyLicenses")
        } header: {
          Text("About")
        } footer: {
          Text("Your tests, pictures and secrets stay on your Macs. This iPhone sees only what you open, over an end-to-end encrypted connection, and proves who it is with a key that never leaves it.")
        }
      }
      .navigationTitle("Settings")
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .confirmationAction) {
          Button("Done", systemImage: "checkmark") { dismiss() }
        }
      }
      .confirmationDialog(
        "Forget “\(forgetting?.name ?? "")”?", isPresented: Binding(get: { forgetting != nil }, set: { if !$0 { forgetting = nil } }),
        titleVisibility: .visible, presenting: forgetting
      ) { host in
        Button("Forget", role: .destructive) {
          Task { await model.forget(host) }
        }
      } message: { _ in
        Text("To use this Mac again, you’ll need to pair with it again.")
      }
    }
  }
}

extension LockTimeout {
  var title: String {
    switch self {
    case .immediately: String(localized: "Immediately")
    case .oneMinute: String(localized: "After 1 Minute")
    case .fiveMinutes: String(localized: "After 5 Minutes")
    case .fifteenMinutes: String(localized: "After 15 Minutes")
    }
  }
}
