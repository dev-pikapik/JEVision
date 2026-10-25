import JEVisionCore
import SwiftUI

/// Settings › Advanced: whether the project's tests start apps as on their first launch, saved in
/// the project's settings file so it holds for everyone who runs them.
struct CleanHomeSection: View {
  @Environment(ProjectStore.self) private var project
  /// What was just saved, shown until the project's settings are read again.
  @State private var saved: Bool?
  @State private var failure: String?

  var body: some View {
    Section {
      if project.rootURL == nil {
        LabeledContent {
          Button("Open Project…", action: project.choose)
        } label: {
          RowLabel(Text("No project is open"), Text("Open the folder with your tests to choose how they start apps."))
        }
        .settingAnchor("cleanHome")
      } else if let problem = project.settingsProblem {
        Label(problem, systemImage: "exclamationmark.triangle.fill")
          .foregroundStyle(.red)
          .textSelection(.enabled)
      } else {
        Toggle(isOn: Binding(get: { saved ?? project.settings.cleanHome }, set: { set(cleanHome: $0) })) {
          RowLabel(
            Text("Start apps as on their first launch"),
            Text(
              "When “Start Fresh” is on in an “Open App” block, the app starts with an empty home folder and none of its settings. When the run ends, its settings come back as they were. Your own files aren’t touched."
            ))
        }
        .settingAnchor("cleanHome")
      }
    } header: {
      Text("Starting apps")
    } footer: {
      Text(
        "Not reset: passwords in the Keychain, the list of recent documents and what the app keeps on its servers or in iCloud. Apps from the App Store and others in the macOS sandbox keep their data where macOS puts it, so for them “Start Fresh” only leaves out the windows from last time. The setting is saved in \(projectSettingsFileName()) in the project folder."
      )
      .font(.callout)
      .foregroundStyle(.secondary)
      .fixedSize(horizontal: false, vertical: true)
    }
    .onChange(of: project.settings) { saved = nil }
    .alert(
      "Can’t change the project settings", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } }),
      presenting: failure
    ) { _ in
      Button("OK") {}
    } message: { Text($0) }
  }

  private func set(cleanHome: Bool) {
    do { saved = try project.setCleanHome(cleanHome)?.cleanHome } catch { failure = ProjectStore.message(error) }
  }
}
