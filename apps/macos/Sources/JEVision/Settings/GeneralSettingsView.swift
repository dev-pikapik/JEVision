import SwiftUI

struct GeneralSettingsView: View {
  @Environment(Updates.self) private var updates
  @AppStorage(Appearance.key) private var appearance = Appearance.system

  private var language: Binding<String> {
    Binding(get: { AppLanguage.shared.saved }, set: { AppLanguage.shared.choose($0) })
  }

  var body: some View {
    Form {
      SettingsHeader(tab: .general, text: Text("How JEVision looks and which language it speaks."))
      Section {
        Picker("Appearance", selection: $appearance) {
          ForEach(Appearance.allCases) { Text($0.title).tag($0) }
        }
        .pickerStyle(.segmented)
        .settingAnchor("appearance")
        Picker("Language", selection: language) {
          Text("System").tag("")
          Divider()
          ForEach(AppLanguage.choices.dropFirst(), id: \.self) { Text(verbatim: AppLanguage.name($0)).tag($0) }
        }
        .settingAnchor("language")
      } footer: {
        VStack(alignment: .leading, spacing: 6) {
          Text("Names that macOS draws itself, like File and Edit in the menu bar, change the next time JEVision opens.")
          Text("JEVision collects no usage data. Nothing leaves this Mac unless a feature you turned on needs it.")
        }
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      }
      UpdatesSection(updates: updates)
      RestoreDefaultsSection(
        message: Text("Appearance and language follow the system again."),
        isDefault: appearance == .system && language.wrappedValue.isEmpty
      ) {
        appearance = .system
        language.wrappedValue = ""
      }
    }
    .formStyle(.grouped)
    .onChange(of: appearance) { appearance.apply() }
  }
}

private struct UpdatesSection: View {
  @Bindable var updates: Updates

  var body: some View {
    Section {
      if updates.isAvailable {
        Toggle("Check for updates automatically", isOn: $updates.checksAutomatically)
          .settingAnchor("updates")
        LabeledContent {
          Button("Check Now") { updates.check() }
            .disabled(!updates.canCheck)
        } label: {
          Text("Version \(Self.version)")
        }
      } else {
        LabeledContent("Version", value: Self.version)
          .settingAnchor("updates")
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
