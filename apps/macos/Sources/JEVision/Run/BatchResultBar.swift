import SwiftUI

/// How Run All or a folder's run went, at the bottom of the window, with the runs a click away.
struct BatchResultBar: View {
  @Environment(RunCenter.self) private var runs
  @Environment(ProjectStore.self) private var project
  @Environment(\.openSettings) private var openSettings
  let result: RunCenter.BatchResult

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      HStack(alignment: .firstTextBaseline, spacing: 10) {
        Image(systemName: symbol)
          .foregroundStyle(color)
          .font(.title3)
          .accessibilityHidden(true)
        Text(RunText.batchOutcome(result))
          .textSelection(.enabled)
          .fixedSize(horizontal: false, vertical: true)
        Spacer(minLength: 12)
        if result.passed + result.failed + result.quarantined + result.unchecked + result.blocked > 0 {
          Button("Show Runs") { project.sidebar = .runs }
            .buttonStyle(.glass)
        }
        Button("Close", systemImage: "xmark", action: runs.dismissBatchResult)
          .labelStyle(.iconOnly)
          .buttonStyle(.borderless)
          .keyboardShortcut(.cancelAction)
          .help(Text("Close"))
      }
      if let problem = result.notificationProblem {
        Divider()
        HStack(spacing: 10) {
          Label {
            Text(problem).textSelection(.enabled)
          } icon: {
            Image(systemName: "bell.slash").foregroundStyle(.red)
          }
          .font(.callout)
          .fixedSize(horizontal: false, vertical: true)
          Spacer(minLength: 12)
          Button("Notification Settings") {
            UserDefaults.standard.set(SettingsTab.notifications.rawValue, forKey: SettingsTab.key)
            openSettings()
          }
          .buttonStyle(.glass)
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
    if result.failed > 0 { return "xmark.square.fill" }
    if result.stopped { return "stop.circle.fill" }
    return result.passed + result.quarantined == result.total ? "checkmark.circle.fill" : "exclamationmark.triangle.fill"
  }

  private var color: Color {
    if result.failed > 0 { return .red }
    if result.stopped { return .secondary }
    return result.passed + result.quarantined == result.total ? .green : .orange
  }
}
