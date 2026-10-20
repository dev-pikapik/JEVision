import AppKit
import SwiftUI

struct SettingsView: View {
  @Environment(PermissionsMonitor.self) private var monitor
  @AppStorage(SettingsTab.key) private var stored = SettingsTab.general
  @State private var model = SettingsModel(selection: SettingsTab.saved)

  var body: some View {
    let tab = model.selection ?? .general
    NavigationSplitView {
      List(selection: $model.selection) {
        if model.query.isEmpty {
          Section { rows([.general]) }
          Section("Testing") { rows([.ai, .permissions, .advanced]) }
          Section("Connections") { rows([.remote, .git, .notifications]) }
        } else {
          results
        }
      }
      .listStyle(.sidebar)
      .navigationSplitViewColumnWidth(min: 220, ideal: 240, max: 320)
    } detail: {
      detail(tab)
        .id(tab)
        .settingsPage()
        .navigationSplitViewColumnWidth(min: 440, ideal: 520)
        .navigationTitle(tab.title)
        .toolbar {
          ToolbarItem(placement: .navigation) {
            ControlGroup {
              Button { model.goBack() } label: { Label("Back", systemImage: "chevron.backward") }
                .keyboardShortcut("[")
                .disabled(model.back.isEmpty)
              Button { model.goForward() } label: { Label("Forward", systemImage: "chevron.forward") }
                .keyboardShortcut("]")
                .disabled(model.forward.isEmpty)
            }
            .controlGroupStyle(.navigation)
          }
        }
    }
    .searchable(text: $model.search, placement: .sidebar)
    .frame(minWidth: SettingsModel.minSize.width, minHeight: SettingsModel.minSize.height)
    .background(SettingsWindowSetup())
    .environment(model)
    // Other windows open a given pane by saving it, then opening Settings.
    .onChange(of: stored) {
      model.search = ""
      model.selection = stored
    }
    .onChange(of: model.selection) {
      if let selection = model.selection { stored = selection }
    }
    .task { await monitor.watch() }
  }

  private func rows(_ tabs: [SettingsTab]) -> some View {
    ForEach(tabs) { tab in
      label(tab).tag(tab)
    }
  }

  private func label(_ tab: SettingsTab) -> some View {
    // The warning sign stays on Permissions until every permission is on.
    Label { Text(tab.title) } icon: { tab.icon(warns: tab == .permissions && !monitor.allGranted) }
  }

  @ViewBuilder
  private var results: some View {
    let query = model.query
    let items = SettingsItem.all.filter { $0.title.localizedStandardContains(query) }
    let tabs = SettingsTab.allCases.filter { tab in
      tab.title.localizedStandardContains(query) || items.contains { $0.tab == tab }
    }
    if tabs.isEmpty {
      Text("No Results")
        .foregroundStyle(.secondary)
        .frame(maxWidth: .infinity)
        .padding(.top, 12)
    }
    ForEach(tabs) { tab in
      label(tab).tag(tab)
      ForEach(items.filter { $0.tab == tab }) { item in
        Button { model.open(item) } label: {
          Text(item.title)
            .foregroundStyle(.secondary)
            .lineLimit(2)
            .padding(.leading, SettingsTab.iconSize + 6)
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
      }
    }
  }

  @ViewBuilder
  private func detail(_ tab: SettingsTab) -> some View {
    switch tab {
    case .general: GeneralSettingsView()
    case .ai: AISettingsView()
    case .permissions: PermissionsSettingsView()
    case .remote: RemoteSettingsView()
    case .git: GitSettingsView()
    case .notifications: NotificationSettingsView()
    case .advanced: AdvancedSettingsView()
    }
  }
}

/// The open Settings pane; set it before opening Settings to show a given one.
enum SettingsTab: String, CaseIterable, Identifiable {
  case general, ai, permissions, remote, git, notifications, advanced

  static let key = "settingsTab"
  static let iconSize: CGFloat = 20

  static var saved: SettingsTab {
    SettingsTab(rawValue: UserDefaults.standard.string(forKey: key) ?? "") ?? .general
  }

  var id: Self { self }

  var title: String {
    switch self {
    case .general: String(localized: "General")
    case .ai: String(localized: "AI")
    case .permissions: String(localized: "Permissions")
    case .remote: String(localized: "iPhone & Remote")
    case .git: String(localized: "Git and GitHub")
    case .notifications: String(localized: "Notifications")
    case .advanced: String(localized: "Advanced")
    }
  }

  func icon(size: CGFloat = iconSize, warns: Bool = false) -> some View {
    let (symbol, color): (String, Color) =
      switch self {
      case .general: ("gearshape.fill", .gray)
      case .ai: ("sparkles", .purple)
      case .permissions: warns ? ("exclamationmark.triangle.fill", .orange) : ("hand.raised.fill", .blue)
      case .remote: ("iphone", .green)
      case .git: ("arrow.triangle.branch", .orange)
      case .notifications: ("bell.badge.fill", .red)
      case .advanced: ("slider.horizontal.3", .indigo)
      }
    return SectionIcon(symbol: symbol, color: color, size: size)
  }
}

/// Which pane is open, the panes visited before it, and what search found.
@MainActor @Observable
final class SettingsModel {
  static let minSize = CGSize(width: 660, height: 460)

  var selection: SettingsTab? {
    didSet {
      guard !navigating, let oldValue, oldValue != selection else { return }
      back.append(oldValue)
      forward = []
    }
  }
  var search = ""
  /// The row search jumped to, lit up for a moment.
  var highlight: String?
  private(set) var back: [SettingsTab] = []
  private(set) var forward: [SettingsTab] = []
  @ObservationIgnored private var navigating = false

  init(selection: SettingsTab) {
    self.selection = selection
  }

  var query: String { search.trimmingCharacters(in: .whitespacesAndNewlines) }

  func goBack() {
    guard let tab = back.popLast() else { return }
    selection.map { forward.append($0) }
    move(to: tab)
  }

  func goForward() {
    guard let tab = forward.popLast() else { return }
    selection.map { back.append($0) }
    move(to: tab)
  }

  private func move(to tab: SettingsTab) {
    navigating = true
    selection = tab
    navigating = false
  }

  func open(_ item: SettingsItem) {
    selection = item.tab
    highlight = nil
    highlight = item.id
    Task {
      try? await Task.sleep(for: .seconds(1.8))
      if highlight == item.id { highlight = nil }
    }
  }
}

/// A setting Settings search can find: its row carries `settingAnchor(id)`.
struct SettingsItem: Identifiable {
  let tab: SettingsTab
  let id: String
  let title: String

  static var all: [SettingsItem] {
    [
      SettingsItem(tab: .general, id: "appearance", title: String(localized: "Appearance")),
      SettingsItem(tab: .general, id: "language", title: String(localized: "Language")),
      SettingsItem(tab: .general, id: "updates", title: String(localized: "Updates")),
      SettingsItem(tab: .ai, id: "provider", title: String(localized: "Provider")),
      SettingsItem(tab: .ai, id: "localOnly", title: String(localized: "Only models on this Mac")),
      SettingsItem(tab: .ai, id: "pictures", title: String(localized: "Understands pictures")),
      SettingsItem(tab: .ai, id: "fallbackProvider", title: String(localized: "Fallback Provider")),
      SettingsItem(tab: .ai, id: "aiSpending", title: String(localized: "Spending")),
      SettingsItem(tab: .ai, id: "aiLimits", title: String(localized: "Limits")),
      SettingsItem(tab: .ai, id: "aiPrices", title: String(localized: "Prices")),
      SettingsItem(tab: .ai, id: "projectLocalOnly", title: String(localized: "Only models on this Mac for this project")),
      SettingsItem(tab: .ai, id: "hiddenAreas", title: String(localized: "Areas always hidden")),
      SettingsItem(tab: .ai, id: "assistantsRun", title: String(localized: "Allow assistants to run tests")),
      SettingsItem(tab: .ai, id: "assistantsCapture", title: String(localized: "Allow assistants to take screenshots")),
      SettingsItem(tab: .ai, id: "connectAssistant", title: String(localized: "Connect an Assistant")),
    ]
      + Permission.allCases.map { SettingsItem(tab: .permissions, id: "permission.\($0.id)", title: $0.title) }
      + [
        SettingsItem(tab: .remote, id: "remoteAccess", title: String(localized: "Allow remote access")),
        SettingsItem(tab: .remote, id: "confirmRuns", title: String(localized: "Ask on this Mac before a remote run")),
        SettingsItem(tab: .remote, id: "keepAwake", title: String(localized: "Keep this Mac awake while remote access is on")),
        SettingsItem(tab: .remote, id: "relay", title: String(localized: "Relay")),
        SettingsItem(tab: .remote, id: "activityLog", title: String(localized: "Activity log")),
        SettingsItem(tab: .git, id: "repository", title: String(localized: "Repository")),
        SettingsItem(tab: .git, id: "branch", title: String(localized: "Branch")),
        SettingsItem(tab: .git, id: "gitHubAccount", title: String(localized: "Account")),
        SettingsItem(tab: .git, id: "gitHubClientID", title: String(localized: "OAuth App Client ID")),
        SettingsItem(tab: .git, id: "pullRequests", title: String(localized: "Pull Requests")),
        SettingsItem(
          tab: .notifications, id: "slackWhen", title: String(localized: "Send a message to \(NotificationsText.name(.slack))")),
        SettingsItem(
          tab: .notifications, id: "telegramWhen",
          title: String(localized: "Send a message to \(NotificationsText.name(.telegram))")),
        SettingsItem(
          tab: .notifications, id: "emailWhen", title: String(localized: "Send a message to \(NotificationsText.name(.email))")),
        SettingsItem(tab: .notifications, id: "unattendedOnly", title: String(localized: "Only for scheduled runs and CI")),
        SettingsItem(tab: .notifications, id: "slack", title: String(localized: "Post to Slack")),
        SettingsItem(tab: .notifications, id: "telegram", title: String(localized: "Post to Telegram")),
        SettingsItem(tab: .notifications, id: "email", title: String(localized: "Send email")),
        SettingsItem(tab: .advanced, id: "retries", title: String(localized: "Retries")),
        SettingsItem(tab: .advanced, id: "commandLine", title: String(localized: "Run tests from Terminal and scripts")),
        SettingsItem(tab: .advanced, id: "continuousIntegration", title: String(localized: "Run tests on every pull request")),
        SettingsItem(tab: .advanced, id: "recordVideo", title: String(localized: "Record video")),
        SettingsItem(tab: .advanced, id: "keepVideos", title: String(localized: "Keep run videos for")),
        SettingsItem(tab: .advanced, id: "logLevel", title: String(localized: "Log detail")),
        SettingsItem(tab: .advanced, id: "showLogs", title: String(localized: "Logs")),
        SettingsItem(tab: .advanced, id: "crashReports", title: String(localized: "Save crash reports")),
        SettingsItem(tab: .advanced, id: "exampleProject", title: String(localized: "Open the example project")),
        SettingsItem(tab: .advanced, id: "vmGuide", title: String(localized: "Run tests in a virtual machine")),
        SettingsItem(tab: .advanced, id: "resetAll", title: String(localized: "Reset all settings")),
      ]
  }
}

/// Makes the Settings window resizable with a unified toolbar, and remembers its size and place.
private struct SettingsWindowSetup: NSViewRepresentable {
  func makeNSView(context: Context) -> NSView { Hook() }
  func updateNSView(_ nsView: NSView, context: Context) {}

  private final class Hook: NSView {
    private static let autosaveName = "Settings Frame"

    override func viewDidMoveToWindow() {
      super.viewDidMoveToWindow()
      guard let window, window.frameAutosaveName != Self.autosaveName else { return }
      window.styleMask.insert([.resizable, .fullSizeContentView])
      window.toolbarStyle = .unified
      window.collectionBehavior.insert(.fullScreenNone)
      window.contentMinSize = SettingsModel.minSize
      if !window.setFrameUsingName(Self.autosaveName) {
        let screen = window.screen ?? NSScreen.main
        window.setContentSize(CGSize(width: 860, height: min(800, (screen?.visibleFrame.height ?? 840) - 40)))
        window.center()
      }
      window.setFrameAutosaveName(Self.autosaveName)
    }
  }
}

private struct PermissionsSettingsView: View {
  @Environment(PermissionsMonitor.self) private var monitor

  var body: some View {
    Form {
      SettingsHeader(
        tab: .permissions, warns: !monitor.allGranted,
        title: monitor.allGranted ? Text("All set") : Text("JEVision needs two permissions"),
        text: monitor.allGranted
          ? Text("JEVision can see other apps and click in them.")
          : Text("Without them, macOS won’t let JEVision see other apps or click in them."))
      PermissionsSection()
    }
    .formStyle(.grouped)
  }
}
