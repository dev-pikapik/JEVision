import JEVisionCore
import JEVisionHands
import SwiftUI

/// Settings of an Ask AI block: whether the screen shows something, or where an element is, and
/// the name its answer is saved as.
struct AskEditor: View {
  @Binding var question: AskQuestion
  @Binding var saveAs: String
  let context: FieldContext
  @AppStorage(AISettings.providerKey) private var aiProvider = ""
  @Environment(ProjectStore.self) private var project
  @Environment(\.openSettings) private var openSettings
  /// JEV has an API key; read from the Keychain when finding is chosen, not on every redraw.
  @State private var finderReady = true

  private enum Mode: Hashable { case isShown, find }

  /// What was written stays when the question changes.
  private var mode: Binding<Mode> {
    Binding(
      get: { if case .find = question { .find } else { .isShown } },
      set: {
        switch ($0, question) {
        case (.find, .isShown(let expect, _, _)): question = .find(description: expect)
        case (.isShown, .find(let description)): question = .isShown(expect: description, provider: "", model: "")
        default: break
        }
      })
  }

  private var text: Binding<String> {
    Binding(
      get: {
        switch question {
        case .isShown(let expect, _, _): expect
        case .find(let description): description
        }
      },
      set: {
        switch question {
        case .isShown(_, let provider, let model): question = .isShown(expect: $0, provider: provider, model: model)
        case .find: question = .find(description: $0)
        }
      })
  }

  /// A model belongs to its provider, so choosing another provider clears it.
  private var provider: Binding<String> {
    Binding(
      get: { if case .isShown(_, let provider, _) = question { provider } else { "" } },
      set: {
        guard case .isShown(let expect, let provider, _) = question, $0 != provider else { return }
        question = .isShown(expect: expect, provider: $0, model: "")
      })
  }

  private var model: Binding<String> {
    Binding(
      get: { if case .isShown(_, _, let model) = question { model } else { "" } },
      set: {
        guard case .isShown(let expect, let provider, _) = question else { return }
        question = .isShown(expect: expect, provider: provider, model: $0)
      })
  }

  private var hasAI: Bool { !aiProvider.isEmpty || project.settings.aiScreen.map { !$0.provider.isEmpty } == true }

  /// The JEV route that finds elements, as the settings name it.
  private var finderName: String { AISettings.provider(AISettings.app.finderID())?.name ?? "JEV" }

  var body: some View {
    Section {
      Picker(selection: mode) {
        Text("Is It on Screen").tag(Mode.isShown)
        Text("Find Element").tag(Mode.find)
      } label: {
        Text("Ask")
      }
      .pickerStyle(.segmented)
      switch question {
      case .isShown:
        TextField(text: text, prompt: Text("The dark theme is on"), axis: .vertical) { Text("Question") }
          .lineLimit(2...5)
        Text("Describe in plain words what the screen may show. An AI model looks at the screen and says how likely it is.")
          .font(.callout)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
        IssueList(issues: context.issues("expect"))
        AIChoiceFields(
          provider: provider, model: model,
          help: "Which AI answers. “From Project or Settings” uses the one the project names, or else the one chosen in Settings → AI.",
          providerIssues: context.issues("provider"), modelIssues: context.issues("model"))
        if !hasAI {
          setUp("Asking AI needs an AI model.", button: "Set Up AI…")
        }
      case .find:
        TextField(text: text, prompt: Text("The blue Save button"), axis: .vertical) { Text("Element") }
          .lineLimit(1...4)
        if finderReady {
          Text("Describe the element in plain words. \(finderName) finds it on the screen. If it isn’t there, the test fails here.")
            .font(.callout)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        } else {
          setUp("Finding an element by its description needs \(finderName) to be set up.", button: "Set Up JEV…")
        }
        IssueList(issues: context.issues("description"))
      }
    }
    .task(id: mode.wrappedValue) {
      guard mode.wrappedValue == .find else { return }
      let ai = AISettings.app
      finderReady = AISettings.provider(ai.finderID()).map(ai.isSetUp) ?? false
    }
    Section {
      TextField(text: $saveAs, prompt: Text(verbatim: mode.wrappedValue == .find ? "button" : "isDark")) { Text("Save As") }
        .font(.body.monospaced())
        .autocorrectionDisabled()
      IssueList(issues: context.issues("saveAs"))
    } footer: {
      Group {
        switch question {
        case .isShown:
          Text("Later blocks read the answer as ${var.NAME}: the probability from 0 to 1 that the screen shows it when JEV answers, or 1 or 0 from an AI that only answers yes or no. An If block can go one way or the other by it.")
        case .find:
          Text("Later blocks read the element’s area as ${var.NAME}. For example, a Click with the target “Area” and ${var.NAME} clicks its middle.")
        }
      }
      .foregroundStyle(.secondary)
    }
  }

  private func setUp(_ reason: LocalizedStringKey, button: LocalizedStringKey) -> some View {
    HStack(alignment: .firstTextBaseline) {
      Text(reason)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      Spacer(minLength: 4)
      Button(button) {
        Profile.current.defaults.set(SettingsTab.ai.rawValue, forKey: SettingsTab.key)
        openSettings()
      }
      .buttonStyle(.link)
    }
    .font(.callout)
  }
}
