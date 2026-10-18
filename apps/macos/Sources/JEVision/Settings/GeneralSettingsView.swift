import SwiftUI

struct GeneralSettingsView: View {
  @Environment(Updates.self) private var updates
  @AppStorage(Appearance.key) private var appearance = Appearance.system
  @State private var language = AppLanguage.saved
  @State private var asksToRestart = false
  @State private var restartFailed = false

  private var needsRestart: Bool { language != AppLanguage.atLaunch }

  var body: some View {
    Form {
      Section {
        PageHeader(
          symbol: "gearshape.fill", color: .gray, title: Text("General"),
          message: Text("How JEVision looks and which language it speaks."))
      }
      Section {
        Picker("Appearance", selection: $appearance) {
          ForEach(Appearance.allCases) { Text($0.title).tag($0) }
        }
        .pickerStyle(.segmented)
        Picker("Language", selection: $language) {
          Text("System").tag("")
          Divider()
          ForEach(AppLanguage.choices.dropFirst(), id: \.self) { Text(verbatim: AppLanguage.name($0)).tag($0) }
        }
        if needsRestart {
          LabeledContent {
            Button("Restart", systemImage: "arrow.clockwise", action: restart)
          } label: {
            Text("Restart JEVision to switch the language")
          }
        }
      } footer: {
        Text("JEVision collects no usage data. Nothing leaves this Mac unless a feature you turned on needs it.")
          .font(.callout)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
      UpdatesSection(updates: updates)
    }
    .formStyle(.grouped)
    .onChange(of: appearance) { appearance.apply() }
    .onChange(of: language) {
      AppLanguage.save(language)
      asksToRestart = needsRestart
    }
    .alert("Restart JEVision now?", isPresented: $asksToRestart) {
      Button("Restart Now", action: restart)
      Button("Later", role: .cancel) {}
    } message: {
      Text("The new language appears after JEVision opens again.")
    }
    .alert("Couldn’t restart JEVision", isPresented: $restartFailed) {
      Button("OK") {}
    } message: {
      Text("Quit JEVision and open it again to switch the language.")
    }
  }

  private func restart() {
    restartFailed = !Relaunch.now()
  }
}

private struct UpdatesSection: View {
  @Bindable var updates: Updates

  var body: some View {
    Section {
      if updates.isAvailable {
        Toggle("Check for updates automatically", isOn: $updates.checksAutomatically)
        LabeledContent {
          Button("Check Now") { updates.check() }
            .disabled(!updates.canCheck)
        } label: {
          Text("Version \(Self.version)")
        }
      } else {
        LabeledContent("Version", value: Self.version)
      }
    } header: {
      Text("Updates")
    } footer: {
      Group {
        if updates.isAvailable {
          Text("JEVision asks before installing an update and installs only updates signed by the JEVision project.")
        } else {
          Text("This build of JEVision doesn’t update itself. Download new versions from the JEVision page on GitHub.")
        }
      }
      .font(.callout)
      .foregroundStyle(.secondary)
      .fixedSize(horizontal: false, vertical: true)
    }
  }

  private static var version: String {
    Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "–"
  }
}
