import JEVisionCore
import JEVisionHands
import SwiftUI

/// Settings → AI → Assistants: what AI assistants such as Claude or Cursor may do through
/// `jevision mcp`, and how to connect one.
struct AssistantsSection: View {
  @Environment(ProjectStore.self) private var project
  @AppStorage(AISettings.assistantsRunKey) private var mayRun = false
  @AppStorage(AISettings.assistantsCaptureKey) private var mayCapture = false
  @AppStorage(AISettings.assistantsAskFirstKey) private var askFirst = true
  @State private var assistants: [ConnectedAssistant] = []
  private let tool = CommandLineTool.bundled

  var body: some View {
    Section {
      Toggle(isOn: $mayRun) {
        RowLabel(Text("Allow assistants to run tests"), Text("Tests open apps, click and type on this Mac. Press Esc twice to stop one."))
      }
      .settingAnchor("assistantsRun")
      Toggle(isOn: $askFirst) {
        RowLabel(
          Text("Ask before an assistant runs a test"),
          Text("You see what it wants to do and allow it or not. Without an answer in a minute, the test isn’t run."))
      }
      .disabled(!mayRun)
      .settingAnchor("assistantsAskFirst")
      Toggle(isOn: $mayCapture) {
        RowLabel(
          Text("Allow assistants to take screenshots"),
          Text("An assistant sees the front window of an app, which may show private information."))
      }
      .settingAnchor("assistantsCapture")
    } header: {
      Text("Assistants")
    } footer: {
      Text("AI assistants on this Mac can always find, read, edit and check the tests of a project and read its reports. They never get the values of secrets or API keys, and can’t open files outside the project.")
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
    }
    Section {
      if assistants.isEmpty {
        Text("No assistant has connected yet. Once you add JEVision to one, it is listed here.")
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
      ForEach(assistants, id: \.name) { AssistantRow(assistant: $0) }
      if assistants.contains(where: { !$0.connected }) {
        Button("Forget the Ones Not Connected") {
          forgetAssistants(folder: AISettings.assistantsFolder.path)
          Task { await reload() }
        }
      }
    } header: {
      Text("Connected Assistants")
    }
    .task {
      while !Task.isCancelled {
        await reload()
        try? await Task.sleep(for: .seconds(2))
      }
    }
    Section {
      if let reason = tool.unavailableReason {
        Text(reason)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
          .settingAnchor("connectAssistant")
      } else {
        Text("Add JEVision to the MCP servers of your assistant, such as Claude, Cursor or Copilot, with this configuration:")
          .fixedSize(horizontal: false, vertical: true)
          .settingAnchor("connectAssistant")
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

  private func reload() async {
    let folder = AISettings.assistantsFolder.path
    let found = await Task.detached { connectedAssistants(folder: folder) }.value
    if found != assistants { assistants = found }
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

/// One assistant that connected: who it is, whether it is connected, and what it did last.
private struct AssistantRow: View {
  let assistant: ConnectedAssistant

  var body: some View {
    LabeledContent {
      if assistant.connected {
        Label("Connected", systemImage: "circle.fill")
          .foregroundStyle(.green)
          .labelStyle(.titleAndIcon)
      } else {
        Text("Not connected")
          .foregroundStyle(.secondary)
      }
    } label: {
      VStack(alignment: .leading, spacing: 2) {
        Text(verbatim: assistant.version.isEmpty ? assistant.title : "\(assistant.title) \(assistant.version)")
        Text("First connected \(Self.when(assistant.firstConnectedAt))")
          .font(.subheadline)
          .foregroundStyle(.secondary)
        Text(lastCall)
          .font(.subheadline)
          .foregroundStyle(.secondary)
      }
      .fixedSize(horizontal: false, vertical: true)
    }
    .accessibilityElement(children: .combine)
  }

  private var lastCall: String {
    guard let call = assistant.lastCall else { return String(localized: "Hasn’t used JEVision yet") }
    return String(localized: "Last: \(Self.action(call.tool)), \(Self.when(call.at))")
  }

  private static func when(_ seconds: Int64) -> String {
    Date(timeIntervalSince1970: TimeInterval(seconds)).formatted(.relative(presentation: .named))
  }

  /// What a tool of the server does, in words for the person; an unknown one keeps its name.
  static func action(_ tool: String) -> String {
    switch tool {
    case "list_tests": String(localized: "looked for tests")
    case "read_test": String(localized: "read a test")
    case "write_test": String(localized: "wrote a test")
    case "add_blocks": String(localized: "added blocks")
    case "remove_block": String(localized: "removed a block")
    case "validate_test": String(localized: "checked a test")
    case "list_block_types": String(localized: "read the guide to blocks")
    case "run_test": String(localized: "ran a test")
    case "get_run": String(localized: "looked at a run")
    case "stop_run": String(localized: "stopped a run")
    case "get_report": String(localized: "read a report")
    case "screenshot": String(localized: "took a screenshot")
    case "draft_bug_report": String(localized: "drafted a bug report")
    case "send_bug_report": String(localized: "sent a bug report")
    default: tool
    }
  }
}
