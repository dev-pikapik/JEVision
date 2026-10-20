import JEVisionCore
import JEVisionHands
import SwiftUI

/// Settings › AI: the AI the open project asks, saved in the project's settings file so everyone
/// who runs its tests gets it. A block of a test can still name its own. Keys stay in the
/// Keychain; the file holds only the provider and the model.
struct ProjectAISection: View {
  @Environment(ProjectStore.self) private var project
  @State private var failure: String?

  var body: some View {
    Section {
      if project.rootURL == nil {
        LabeledContent {
          Button("Open Project…", action: project.choose)
        } label: {
          RowLabel(Text("No project is open"), Text("Open the folder with your tests to choose the AI it asks."))
        }
        .settingAnchor("projectAIScreen")
      } else if let problem = project.settingsProblem {
        Label(problem, systemImage: "exclamationmark.triangle.fill")
          .foregroundStyle(.red)
          .textSelection(.enabled)
      } else {
        choice(.screen, title: Text("Checks by Meaning"), anchor: "projectAIScreen")
        choice(.text, title: Text("Writing and Failure Analysis"), anchor: "projectAIText")
      }
    } header: {
      Text("AI for this project")
    } footer: {
      Text(
        "“Checks by Meaning” judge a screen from a sentence, with or without a picture. “Writing and Failure Analysis” drafts tests and explains why one failed. Finding an element by its description always uses JEV. A block can name its own AI for a check. The choice is saved in \(projectSettingsFileName()) in the project folder, without any key."
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

  private func current(_ task: AiTask) -> AiChoice? {
    task == .screen ? project.settings.aiScreen : project.settings.aiText
  }

  @ViewBuilder
  private func choice(_ task: AiTask, title: Text, anchor: String) -> some View {
    let chosen = current(task)
    // A text task can't use JEV, which only reads screenshots.
    let providers = AISettings.providers.filter { task == .screen || $0.id != AISettings.finder }
    Picker(selection: Binding(get: { chosen?.provider ?? "" }, set: { set(task, provider: $0, model: "") })) {
      Text("From Settings").tag("")
      Divider()
      ForEach(providers, id: \.id) { Text(verbatim: $0.name).tag($0.id) }
    } label: {
      title
    }
    .settingAnchor(anchor)
    if let chosen, let provider = AISettings.provider(chosen.provider) {
      TextField(
        text: Binding(get: { chosen.model }, set: { set(task, provider: chosen.provider, model: $0) }),
        prompt: Text(verbatim: AISettings(.standard).model(for: provider))
      ) { Text("Model for \(provider.name)") }
    }
  }

  private func set(_ task: AiTask, provider: String, model: String) {
    let choice = provider.isEmpty ? nil : AiChoice(provider: provider, model: model)
    guard choice != current(task) else { return }
    do { try project.setAi(task, choice) } catch { failure = ProjectStore.message(error) }
  }
}
