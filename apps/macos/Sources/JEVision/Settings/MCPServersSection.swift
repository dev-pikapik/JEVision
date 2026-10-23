import JEVisionCore
import JEVisionHands
import SwiftUI

/// An MCP server whose tools the AI may use, such as an issue tracker: a program JEVision starts,
/// or a web address. Its access token is kept in the Keychain, not here.
struct MCPServerEntry: Codable, Identifiable, Hashable {
  enum Kind: String, Codable { case command, http }

  var id = UUID()
  var name = ""
  var kind = Kind.command
  var command = ""
  var url = ""

  /// The server as the core connects to it, with its token when it has one.
  var server: McpServer {
    switch kind {
    case .command: McpServer(name: name, transport: .command(command: command))
    case .http: McpServer(name: name, transport: .http(url: url, token: MCPServers.token(of: id)))
    }
  }

  /// What starts the server or where it is.
  var location: String { kind == .command ? command : url }
}

/// The MCP servers of Settings → AI, kept in the app's settings.
enum MCPServers {
  static let key = "ai.mcpServers"
  static let tokenService = "JEVision MCP"

  static func load(from defaults: UserDefaults = Profile.current.defaults) -> [MCPServerEntry] {
    defaults.data(forKey: key).flatMap { try? JSONDecoder().decode([MCPServerEntry].self, from: $0) } ?? []
  }

  static func save(_ entries: [MCPServerEntry], to defaults: UserDefaults = Profile.current.defaults) {
    defaults.set(try? JSONEncoder().encode(entries), forKey: key)
  }

  /// The servers the AI may use now.
  static var all: [McpServer] { load().map(\.server) }

  static func token(of id: UUID) -> String? {
    SecretStore.value(id.uuidString, in: tokenService)
  }
}

/// Settings → AI → Tools: the MCP servers the AI may use, and what tools each one has.
struct MCPServersSection: View {
  @State private var entries = MCPServers.load()
  @State private var editing: MCPServerEntry?
  @State private var states: [UUID: Connection] = [:]

  private enum Connection: Equatable {
    case connecting
    case connected([McpTool])
    case failed(String)
  }

  var body: some View {
    Section {
      ForEach(entries) { entry in
        row(entry)
      }
      Button("Add Server…", systemImage: "plus") { editing = MCPServerEntry() }
    } header: {
      Text("Tools")
    } footer: {
      Text("When the AI writes a test or analyzes a failure, it can use the tools of these MCP servers, such as finding an issue in a tracker. It asks you before every tool that may change something.")
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
    }
    .sheet(item: $editing) { entry in
      MCPServerSheet(entry: entry, taken: Set(entries.filter { $0.id != entry.id }.map(\.name))) { saved in
        if let index = entries.firstIndex(where: { $0.id == saved.id }) {
          entries[index] = saved
        } else {
          entries.append(saved)
        }
        MCPServers.save(entries)
        states[saved.id] = nil
      }
    }
  }

  private func row(_ entry: MCPServerEntry) -> some View {
    VStack(alignment: .leading, spacing: 6) {
      LabeledContent {
        HStack {
          Button("Connect") { connect(entry) }
            .disabled(states[entry.id] == .connecting)
            .accessibilityHint(Text("Starts the server and lists its tools."))
          Menu {
            Button("Edit…", systemImage: "pencil") { editing = entry }
            Button("Remove", systemImage: "trash", role: .destructive) { remove(entry) }
          } label: {
            Label("More", systemImage: "ellipsis.circle")
          }
          .menuIndicator(.hidden)
          .labelStyle(.iconOnly)
          .fixedSize()
          .accessibilityLabel(Text("More for \(entry.name)"))
        }
      } label: {
        Text(verbatim: entry.name)
        Text(verbatim: entry.location)
      }
      switch states[entry.id] {
      case nil:
        EmptyView()
      case .connecting:
        HStack {
          ProgressView().controlSize(.small)
          Text("Connecting…")
        }
      case .connected(let tools):
        Label {
          Text(tools.isEmpty ? String(localized: "Connected. The server has no tools.") : String(localized: "Connected. Tools: \(tools.map(\.title).formatted(.list(type: .and)))"))
        } icon: {
          Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
        }
        .fixedSize(horizontal: false, vertical: true)
      case .failed(let message):
        Label {
          Text(message)
        } icon: {
          Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.red)
        }
        .textSelection(.enabled)
        .fixedSize(horizontal: false, vertical: true)
      }
    }
  }

  private func connect(_ entry: MCPServerEntry) {
    states[entry.id] = .connecting
    let server = entry.server
    Task {
      let result = await Task.detached { Result { try mcpTools(server: server) } }.value
      guard states[entry.id] == .connecting else { return }
      let state: Connection
      switch result {
      case .success(let tools): state = .connected(tools)
      case .failure(let error): state = .failed(AIText.error(error))
      }
      states[entry.id] = state
      switch state {
      case .connected(let tools):
        AccessibilityNotification.Announcement(String(localized: "\(entry.name) is connected with \(tools.count) tools.")).post()
      case .failed(let message):
        AccessibilityNotification.Announcement(message).post()
      case .connecting: break
      }
    }
  }

  private func remove(_ entry: MCPServerEntry) {
    entries.removeAll { $0.id == entry.id }
    MCPServers.save(entries)
    states[entry.id] = nil
    try? SecretStore.remove(entry.id.uuidString, in: MCPServers.tokenService)
  }
}

/// Adds or changes an MCP server.
private struct MCPServerSheet: View {
  @State var entry: MCPServerEntry
  /// Names of the other servers: the AI tells servers apart by name.
  let taken: Set<String>
  let saved: (MCPServerEntry) -> Void
  @Environment(\.dismiss) private var dismiss
  @State private var token = ""
  @State private var hasToken = false
  @State private var error: String?

  private var name: String { entry.name.trimmingCharacters(in: .whitespaces) }
  private var location: String { entry.location.trimmingCharacters(in: .whitespaces) }
  private var canSave: Bool { !name.isEmpty && !taken.contains(name) && !location.isEmpty }

  var body: some View {
    VStack(alignment: .leading, spacing: 16) {
      Text("MCP Server")
        .font(.title2.bold())
        .accessibilityAddTraits(.isHeader)
      Form {
        TextField(text: $entry.name, prompt: Text("Tracker")) { Text("Name") }
        if taken.contains(name) {
          Text("Another server has this name.").foregroundStyle(.red).font(.callout)
        }
        Picker("Connect by", selection: $entry.kind) {
          Text("Command").tag(MCPServerEntry.Kind.command)
          Text("Web Address").tag(MCPServerEntry.Kind.http)
        }
        .pickerStyle(.segmented)
        switch entry.kind {
        case .command:
          TextField(text: $entry.command, prompt: Text(verbatim: "npx -y @example/tracker-mcp")) { Text("Command") }
            .font(.body.monospaced())
          Text("JEVision runs the command in your login shell and talks to it through its input and output.")
            .font(.callout)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        case .http:
          TextField(text: $entry.url, prompt: Text(verbatim: "https://tracker.example.com/mcp")) { Text("Address") }
          SecureField(text: $token, prompt: Text(hasToken ? "Saved — type to replace" : "Only if the server asks for one")) {
            Text("Access Token")
          }
          if hasToken {
            Button("Remove Token", role: .destructive) {
              do {
                try SecretStore.remove(entry.id.uuidString, in: MCPServers.tokenService)
                hasToken = false
              } catch {
                self.error = error.localizedDescription
              }
            }
          }
        }
      }
      .formStyle(.columns)
      if let error {
        Text(error).foregroundStyle(.red).font(.callout)
      }
      HStack {
        Spacer()
        Button("Cancel", role: .cancel) { dismiss() }
          .keyboardShortcut(.cancelAction)
        Button("Save") { save() }
          .keyboardShortcut(.defaultAction)
          .disabled(!canSave)
      }
    }
    .padding(20)
    .frame(width: 460)
    .onAppear { hasToken = SecretStore.isInKeychain(entry.id.uuidString, in: MCPServers.tokenService) }
  }

  private func save() {
    var entry = entry
    entry.name = name
    entry.command = entry.command.trimmingCharacters(in: .whitespaces)
    entry.url = entry.url.trimmingCharacters(in: .whitespaces)
    do {
      if entry.kind == .http, !token.isEmpty {
        try SecretStore.save(token, for: entry.id.uuidString, in: MCPServers.tokenService)
      }
      saved(entry)
      dismiss()
    } catch {
      self.error = error.localizedDescription
    }
  }
}
