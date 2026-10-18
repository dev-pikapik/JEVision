import JEVisionCore
import JEVisionHands
import SwiftUI

struct AdvancedSettingsView: View {
  @AppStorage(VideoSettings.modeKey) private var mode = VideoSettings.defaultMode.name
  @AppStorage(VideoSettings.keepDaysKey) private var keepDays = VideoSettings.defaultKeepDays

  var body: some View {
    Form {
      Section {
        PageHeader(
          symbol: "slider.horizontal.3", color: .gray, title: Text("Advanced"),
          message: Text("Command line, CI, run videos and other settings most people never change."))
      }
      CommandLineSection()
      ContinuousIntegrationSection()
      Section {
        Picker("Record video", selection: $mode) {
          ForEach(VideoMode.all, id: \.name) { Text(Self.title($0)).tag($0.name) }
        }
        Picker("Keep run videos for", selection: $keepDays) {
          ForEach(VideoSettings.keepChoices, id: \.self) { Text(Self.title(days: $0)).tag($0) }
        }
        .disabled(mode == VideoMode.off.name)
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
      ResetSettingsSection()
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
