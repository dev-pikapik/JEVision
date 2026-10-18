import JEVisionCore
import SwiftUI

/// Settings › AI: what the open project lets go to AI models, saved in the project's settings
/// file so it holds for everyone who runs its tests.
struct ProjectPrivacySection: View {
  @Environment(ProjectStore.self) private var project
  @State private var failure: String?
  @Environment(\.openWindow) private var openWindow

  var body: some View {
    Section {
      if project.rootURL == nil {
        LabeledContent {
          Button("Open Project…", action: project.choose)
        } label: {
          Text("No project is open")
          Text("Open the folder with your tests to keep its screenshots on this Mac.")
        }
      } else if let problem = project.settingsProblem {
        Label(problem, systemImage: "exclamationmark.triangle.fill")
          .foregroundStyle(.red)
          .textSelection(.enabled)
      } else {
        Toggle(isOn: Binding(get: { project.settings.localOnly }, set: { set(localOnly: $0) })) {
          Text("Only models on this Mac for this project")
          Text("Its screenshots never go to a cloud service or to AI assistants, whatever each person’s settings say.")
        }
        LabeledContent("Areas always hidden") {
          HStack {
            if project.settings.hide.isEmpty {
              Text("None")
            } else {
              Text(project.settings.hide.count, format: .number)
            }
            Button("Edit…") { openWindow(id: SceneID.hiddenAreas) }
              .accessibilityLabel(Text("Edit Areas Always Hidden"))
          }
        }
      }
    } header: {
      Text("This project")
    } footer: {
      Text(
        "Before a screenshot goes to an AI model, JEVision paints over password fields, the areas each check hides, and the areas listed under “hide” in \(projectSettingsFileName()) in the project folder."
      )
      .font(.callout)
      .foregroundStyle(.secondary)
      .fixedSize(horizontal: false, vertical: true)
    }
    .alert(
      "Can’t change the project settings", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } }),
      presenting: failure
    ) { _ in
      Button("OK") {}
    } message: { Text($0) }
  }

  private func set(localOnly: Bool) {
    do { try project.setLocalOnly(localOnly) } catch { failure = ProjectStore.message(error) }
  }
}
