import AppKit
import JEVisionHands
import SwiftUI

/// Settings → Updates: which version this is, and how JEVision updates itself.
struct UpdatesSettingsView: View {
  @Environment(Updates.self) private var updates
  private let version = AppVersion()

  var body: some View {
    @Bindable var updates = updates
    Form {
      SettingsHeader(tab: .updates, text: Text("Which version of JEVision this is, and how it gets the next one."))
      Section {
        LabeledContent {
          if updates.isAvailable {
            Button("Check Now") { updates.check() }
              .disabled(!updates.canCheck)
          }
        } label: {
          RowLabel(Text("Version \(version.version) (\(version.build))"), lastCheck)
        }
        .settingAnchor("updates")
      } footer: {
        footer(
          updates.isAvailable
            ? Text("JEVision installs only updates signed by the JEVision project, and never an older version than the one you have.")
            : Profile.current.isolated
              ? Text("Not available in a separate profile: updates are for the copy of JEVision you use every day.")
              : Text("This build of JEVision doesn’t update itself. Download new versions from the JEVision page on GitHub."))
      }
      if updates.isAvailable {
        Section {
          Toggle(isOn: $updates.checksAutomatically) {
            RowLabel(Text("Check for updates automatically"), Text("When JEVision opens and once a day. It asks before installing."))
          }
          .settingAnchor("updatesCheck")
          Toggle(isOn: $updates.downloadsAutomatically) {
            RowLabel(
              Text("Download and install updates automatically"),
              Text("JEVision downloads an update in the background and installs it when you quit."))
          }
          .disabled(!updates.checksAutomatically)
          .settingAnchor("updatesInstall")
          Toggle(isOn: $updates.includesBetas) {
            RowLabel(
              Text("Get beta versions"),
              Text("Try new features before the release. A beta can have bugs. When you turn this off, you keep the beta until the next release replaces it."))
          }
          .settingAnchor("updatesBetas")
        }
        Section {
          LabeledContent {
            Button("Show in Finder") {
              try? FileManager.default.createDirectory(at: UpdateDataGuard.backups, withIntermediateDirectories: true)
              NSWorkspace.shared.open(UpdateDataGuard.backups)
            }
          } label: {
            RowLabel(
              Text("Copies made before updates"),
              Text("Before an update that stores tests and settings in a new way, JEVision saves a copy of them here. Copies stay until you delete them."))
          }
          .settingAnchor("updatesBackups")
        }
        RestoreDefaultsSection(
          message: Text("JEVision checks for updates on its own again, asks before installing them and offers no beta versions."),
          isDefault: updates.checksAutomatically && !updates.downloadsAutomatically && !updates.includesBetas
        ) {
          updates.checksAutomatically = true
          updates.downloadsAutomatically = false
          updates.includesBetas = false
        }
      }
    }
    .formStyle(.grouped)
  }

  private var lastCheck: Text? {
    guard updates.isAvailable else { return nil }
    guard let date = updates.lastCheck else { return Text("Not checked yet") }
    return Text("Last checked: \(date.formatted(date: .abbreviated, time: .shortened))")
  }

  private func footer(_ text: Text) -> some View {
    text
      .font(.callout)
      .foregroundStyle(.secondary)
      .fixedSize(horizontal: false, vertical: true)
  }
}
