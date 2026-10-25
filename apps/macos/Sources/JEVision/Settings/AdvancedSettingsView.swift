import JEVisionCore
import JEVisionHands
import SwiftUI

struct AdvancedSettingsView: View {
  @AppPreference(VideoSettings.modeKey) private var mode = VideoSettings.defaultMode.name
  @AppPreference(VideoSettings.keepDaysKey) private var keepDays = VideoSettings.defaultKeepDays
  @AppPreference(AppLog.levelKey) private var logLevel = LogLevel.default.rawValue
  @AppPreference(CrashReports.enabledKey) private var savesCrashes = false

  var body: some View {
    Form {
      SettingsHeader(tab: .advanced, text: Text("Command line, CI, run videos, logs and other settings most people never change."))
      RetriesSection()
      CleanHomeSection()
      FileFormatSection()
      CommandLineSection()
      ContinuousIntegrationSection()
      Section {
        Picker("Record video", selection: $mode) {
          ForEach(VideoMode.all, id: \.name) { Text(Self.title($0)).tag($0.name) }
        }
        .settingAnchor("recordVideo")
        Picker("Keep run videos for", selection: $keepDays) {
          ForEach(VideoSettings.keepChoices, id: \.self) { Text(Self.title(days: $0)).tag($0) }
        }
        .disabled(mode == VideoMode.off.name)
        .settingAnchor("keepVideos")
      } header: {
        Text("Run videos")
      } footer: {
        Text(
          "A video shows only the app under test, with a mark for each step in the report. Videos are saved with the run reports in the project’s .jevision folder; older ones are deleted after the next run."
        )
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      }
      LogsSection(level: $logLevel)
      CrashReportsSection(isOn: $savesCrashes)
      GuidesSection()
      ResetSettingsSection()
      RestoreDefaultsSection(
        message: Text("Run videos are recorded only when a test fails and kept for 30 days, the log says what JEVision does, and crash reports aren’t saved. Retries and quarantine stay: they belong to the project."),
        isDefault: mode == VideoSettings.defaultMode.name && keepDays == VideoSettings.defaultKeepDays
          && logLevel == LogLevel.default.rawValue && !savesCrashes
      ) {
        mode = VideoSettings.defaultMode.name
        keepDays = VideoSettings.defaultKeepDays
        logLevel = LogLevel.default.rawValue
        savesCrashes = false
      }
    }
    .formStyle(.grouped)
  }

  static func title(_ mode: VideoMode) -> String {
    switch mode {
    case .off: String(localized: "Never")
    case .onFailure: String(localized: "Only when a test fails")
    case .always: String(localized: "Always")
    }
  }

  static func title(days: Int) -> String {
    days > 0 ? String(localized: "\(days) days") : String(localized: "Forever")
  }
}
