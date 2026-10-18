import AppKit
import JEVisionCore
import SwiftUI

/// How the last run ended, at the bottom of the editor, with the report a click away.
struct RunResultBar: View {
  @Environment(RunCenter.self) private var runs
  @Environment(ProjectStore.self) private var project
  @Environment(\.openSettings) private var openSettings
  let result: RunCenter.Result
  /// Selects the block the run failed at and shows why; `nil` when no block failed.
  var showFailedBlock: (() -> Void)?
  @State private var gitIgnoreError: String?

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      HStack(alignment: .firstTextBaseline, spacing: 10) {
        Image(systemName: symbol)
          .foregroundStyle(color)
          .font(.title3)
          .accessibilityHidden(true)
        Text(result.message)
          .textSelection(.enabled)
          .fixedSize(horizontal: false, vertical: true)
        Spacer(minLength: 12)
        if result.permission != nil {
          Button("Open Permissions") {
            UserDefaults.standard.set(SettingsTab.permissions.rawValue, forKey: SettingsTab.key)
            openSettings()
          }
          .buttonStyle(.glassProminent)
        }
        if result.needsAISetup {
          Button("Open AI Settings") {
            UserDefaults.standard.set(SettingsTab.ai.rawValue, forKey: SettingsTab.key)
            openSettings()
          }
          .buttonStyle(.glassProminent)
        }
        if let showFailedBlock {
          Button("Show Failed Block", action: showFailedBlock)
            .buttonStyle(.glassProminent)
        }
        if let run = result.run {
          Button("Open Report") { project.showRun(run) }
            .buttonStyle(.glass)
        }
        Button("Close", systemImage: "xmark", action: runs.dismissResult)
          .labelStyle(.iconOnly)
          .buttonStyle(.borderless)
          .keyboardShortcut(.cancelAction)
          .help(Text("Close"))
      }
      if result.offersGitIgnore {
        Divider()
        HStack(spacing: 10) {
          Text("Runs are saved in the .jevision folder of the project. Keep them out of git?")
            .font(.callout)
            .fixedSize(horizontal: false, vertical: true)
          Spacer(minLength: 12)
          Button("Not Now", action: runs.declineGitIgnore)
            .buttonStyle(.glass)
          Button("Add to .gitignore") {
            do {
              try runs.addToGitIgnore()
            } catch {
              gitIgnoreError = error.localizedDescription
            }
          }
          .buttonStyle(.glass)
        }
        if let gitIgnoreError {
          Text(gitIgnoreError).font(.callout).foregroundStyle(.red)
        }
      }
    }
    .padding(.horizontal, 16)
    .padding(.vertical, 12)
    .frame(maxWidth: 720)
    .adaptiveGlass(tint: color.opacity(0.15), in: .rect(cornerRadius: 18))
    .padding()
    .accessibilityElement(children: .contain)
    .accessibilityLabel(Text("Run result"))
  }

  private var symbol: String {
    switch result.verdict {
    case .passed: "checkmark.circle.fill"
    case .cancelled: "stop.circle.fill"
    case .failed, nil: "xmark.square.fill"
    }
  }

  private var color: Color {
    switch result.verdict {
    case .passed: .green
    case .cancelled: .secondary
    case .failed, nil: .red
    }
  }
}
