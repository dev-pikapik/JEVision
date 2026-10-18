import JEVisionHands
import SwiftUI

/// Settings → AI → Assistants: what AI assistants such as Claude or Cursor may do through
/// `jevision mcp`, and how to connect one.
struct AssistantsSection: View {
  @Environment(ProjectStore.self) private var project
  @AppStorage(AISettings.assistantsRunKey) private var mayRun = false
  @AppStorage(AISettings.assistantsCaptureKey) private var mayCapture = false
  private let tool = CommandLineTool.bundled

  var body: some View {
    Section {
      Toggle(isOn: $mayRun) {
        Text("Allow assistants to run tests")
        Text("Tests open apps, click and type on this Mac. Press Esc twice to stop one.")
      }
      Toggle(isOn: $mayCapture) {
        Text("Allow assistants to take screenshots")
        Text("An assistant sees the front window of an app, which may show private information.")
      }
    } header: {
      Text("Assistants")
    } footer: {
      Text("AI assistants on this Mac can always find, read, edit and check the tests of a project and read its reports. They never get the values of secrets or API keys, and can’t open files outside the project.")
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
    }
    Section {
      if let reason = tool.unavailableReason {
        Text(reason)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      } else {
        Text("Add JEVision to the MCP servers of your assistant, such as Claude, Cursor or Copilot, with this configuration:")
          .fixedSize(horizontal: false, vertical: true)
        CommandText(configuration)
      }
    } header: {
      Text("Connect an Assistant")
    } footer: {
      if tool.unavailableReason == nil {
        Group {
          if project.rootURL == nil {
            Text("Open a project to name it here. Without --project, the assistant sees the folder it starts jevision in.")
          } else {
            Text("The assistant sees the project open now. To give it another one, change the folder after --project.")
          }
        }
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      }
    }
  }

  /// The `mcpServers` entry most assistants read, with the installed command when there is one.
  private var configuration: String {
    var command = tool.tool.path
    if case .installed(let link) = tool.state() { command = link.path }
    var args = ["mcp"]
    if let root = project.rootURL { args += ["--project", root.path] }
    let entry: [String: Any] = ["mcpServers": ["jevision": ["command": command, "args": args]]]
    let data = try? JSONSerialization.data(withJSONObject: entry, options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes])
    return data.flatMap { String(data: $0, encoding: .utf8) } ?? ""
  }
}
