import JEVisionCore
import JEVisionHands
import SwiftUI

/// Settings › Notifications: messages about the open project's runs to Slack, Telegram or
/// email. The settings are saved in the project folder for everyone who runs its tests; the
/// webhook, bot token and mail password stay in this Mac's Keychain.
struct NotificationSettingsView: View {
  @Environment(ProjectStore.self) private var project
  @State private var saved: NotificationSettings?
  @State private var draft = NotificationsDraft()
  @State private var loadProblem: String?
  @State private var failure: String?
  @State private var diagnostics: [Issue] = []
  @State private var settingSecret: NotifyChannel?
  /// Bumped after a secret is saved or removed, to read the statuses again.
  @State private var secretsVersion = 0
  @State private var testing: NotifyChannel?
  @State private var tested: (channel: NotifyChannel, problem: String?)?
  @State private var chatSearch = ChatSearch.idle

  var body: some View {
    Form {
      SettingsHeader(
        tab: .notifications, text: Text("A message to Slack, Telegram or email when a test fails, or after every run."))
      if let root = project.rootURL {
        if let loadProblem {
          Section {
            Label(loadProblem, systemImage: "exclamationmark.triangle.fill")
              .foregroundStyle(.red)
              .textSelection(.enabled)
            Button("Try Again") { load(root) }
          }
        } else if saved != nil {
          if !diagnostics.isEmpty {
            Section {
              ForEach(Array(diagnostics.enumerated()), id: \.offset) { _, issue in
                Label(describeIssue(issue: issue), systemImage: "exclamationmark.triangle")
                  .foregroundStyle(issue.severity == .error ? Color.red : Color.orange)
              }
            }
          }
          form(root)
        }
      } else {
        Section {
          LabeledContent {
            Button("Open Project…", action: project.choose)
          } label: {
            Text("No project is open")
            Text("Open the folder with your tests to send messages about its runs.")
          }
        }
      }
    }
    .formStyle(.grouped)
    .task(id: project.rootURL) {
      if let root = project.rootURL { load(root) }
    }
    .sheet(item: $settingSecret) { channel in
      if let root = project.rootURL {
        let item = notificationKeychainItem(project: root.path, channel: channel)
        SetSecretSheet(
          name: item.account, title: secretTitle(channel), message: secretMessage(channel), service: item.service
        ) { secretsVersion += 1 }
      }
    }
    .alert(
      "Can’t change the notifications", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } }),
      presenting: failure
    ) { _ in
      Button("OK") {}
    } message: { Text($0) }
  }

  @ViewBuilder private func form(_ root: URL) -> some View {
    Section {
      Toggle(isOn: $draft.unattendedOnly) {
        Text("Only for scheduled runs and CI")
        Text("Runs you start yourself in JEVision send nothing.")
      }
      .settingAnchor("unattendedOnly")
    } footer: {
      help(
        "Run All and scheduled runs of several tests send one summary message, not one message per test. A test in quarantine sends a message about a failure only with “After every run”. A stopped run sends nothing."
      )
    }

    Section {
      Toggle("Post to Slack", isOn: $draft.slack)
        .settingAnchor("slack")
      if draft.slack {
        whenPicker(.slack, selection: $draft.slackWhen)
          .settingAnchor("slackWhen")
        secretRow(.slack, root: root, label: Text("Webhook"))
        testRow(.slack)
      }
    } header: {
      Text(verbatim: "Slack")
    } footer: {
      if draft.slack {
        help(
          "In Slack, create an app at api.slack.com/apps, turn on Incoming Webhooks, add a webhook for the channel and copy its URL, which starts with https://hooks.slack.com/."
        )
      }
    }

    Section {
      Toggle("Post to Telegram", isOn: $draft.telegram)
        .settingAnchor("telegram")
      if draft.telegram {
        whenPicker(.telegram, selection: $draft.telegramWhen)
          .settingAnchor("telegramWhen")
        secretRow(.telegram, root: root, label: Text("Bot token"))
        TextField(text: $draft.chat, prompt: Text(verbatim: "-1001234567890")) { Text("Chat ID") }
          .onSubmit { save(root) }
        chatFinder
        testRow(.telegram)
      }
    } header: {
      Text(verbatim: "Telegram")
    } footer: {
      if draft.telegram {
        help(
          "Make a bot with @BotFather and copy its token. Add the bot to the group or channel, then use Find Chat, or type the chat’s ID, such as -1001234567890, or @name for a public channel."
        )
      }
    }

    Section {
      Toggle("Send email", isOn: $draft.email)
        .settingAnchor("email")
      if draft.email {
        whenPicker(.email, selection: $draft.emailWhen)
          .settingAnchor("emailWhen")
        TextField(text: $draft.server, prompt: Text(verbatim: "smtp.gmail.com")) { Text("Mail server") }
        TextField(text: $draft.port, prompt: Text(verbatim: "465")) { Text("Port") }
        Picker(selection: $draft.security) {
          Text(verbatim: "TLS").tag(MailSecurity.tls)
          Text(verbatim: "STARTTLS").tag(MailSecurity.startTls)
        } label: {
          Text("Encryption")
          Text("TLS usually goes with port 465, STARTTLS with 587. Mail is never sent unencrypted.")
        }
        TextField(text: $draft.user, prompt: Text(verbatim: "qa@example.com")) {
          Text("User name")
          Text("Leave it empty if the server takes mail without signing in.")
        }
        if !draft.user.trimmingCharacters(in: .whitespaces).isEmpty {
          secretRow(.email, root: root, label: Text("Password"))
        }
        TextField(text: $draft.from, prompt: Text(verbatim: "JEVision <qa@example.com>")) { Text("From") }
        TextField(text: $draft.to, prompt: Text(verbatim: "team@example.com, anna@example.com"), axis: .vertical) {
          Text("To")
        }
        .lineLimit(1...3)
        testRow(.email)
      }
    } header: {
      Text("Email")
    } footer: {
      if draft.email {
        help("For Gmail and iCloud Mail, make an app password in your account’s security settings and use it here.")
      }
    }

    Section {
      LabeledContent {
        HStack {
          Button("Revert") { draft = NotificationsDraft(saved ?? NotificationSettings.off) }
          Button("Save") { save(root) }
            .buttonStyle(.glassProminent)
            .keyboardShortcut("s")
        }
        .disabled(!hasChanges)
      } label: {
        Text(hasChanges ? "Not saved yet" : "Saved")
        Text("In \(notificationSettingsFileName()) in the project folder, so everyone who runs the tests gets the messages.")
      }
    } footer: {
      help(
        "The webhook, bot token and password stay in the Keychain on this Mac. On CI, set them in the environment variables JEVISION_SLACK_WEBHOOK, JEVISION_TELEGRAM_TOKEN and JEVISION_SMTP_PASSWORD. To run tests without messages, add --no-notify to jevision run."
      )
    }
  }

  /// The choice of which runs a channel is told about, with what it means under it.
  private func whenPicker(_ channel: NotifyChannel, selection: Binding<NotifyWhen>) -> some View {
    Picker(selection: selection) {
      Text("When a test fails").tag(NotifyWhen.failed)
      Text("When a test fails or passes again").tag(NotifyWhen.failedOrFixed)
      Text("When a test starts or stops failing").tag(NotifyWhen.changed)
      Text("After every run").tag(NotifyWhen.every)
    } label: {
      Text("Send a message")
      Text(NotificationsText.explanation(selection.wrappedValue))
    }
    .accessibilityLabel(Text("Send a message to \(NotificationsText.name(channel))"))
    .accessibilityHint(Text(NotificationsText.explanation(selection.wrappedValue)))
  }

  private var hasChanges: Bool {
    guard let saved else { return false }
    return draft != NotificationsDraft(saved)
  }

  private func help(_ text: LocalizedStringKey) -> some View {
    Text(text)
      .font(.callout)
      .foregroundStyle(.secondary)
      .fixedSize(horizontal: false, vertical: true)
  }

  @ViewBuilder private func secretRow(_ channel: NotifyChannel, root: URL, label: Text) -> some View {
    let _ = secretsVersion
    let item = notificationKeychainItem(project: root.path, channel: channel)
    let variable = notificationSecretVariable(channel: channel)
    let fromEnvironment = SecretStore.isInEnvironment(variable)
    let inKeychain = SecretStore.isInKeychain(item.account, in: item.service)
    let isSet = fromEnvironment || inKeychain
    LabeledContent {
      HStack {
        Label {
          Text(isSet ? "Set" : "Not set")
        } icon: {
          Image(systemName: isSet ? "checkmark.circle.fill" : "circle.dashed")
            .foregroundStyle(isSet ? .green : .secondary)
        }
        if inKeychain, !fromEnvironment {
          Button("Remove", role: .destructive) {
            do {
              try SecretStore.remove(item.account, in: item.service)
              secretsVersion += 1
            } catch {
              failure = error.localizedDescription
            }
          }
          .accessibilityLabel(Text("Remove \(secretName(channel))"))
        }
        Button("Set…") { settingSecret = channel }
          .disabled(fromEnvironment)
          .accessibilityLabel(Text("Set \(secretName(channel))"))
      }
    } label: {
      label
      if fromEnvironment {
        Text("From the environment variable \(variable)")
      } else {
        Text("Kept in the Keychain on this Mac, not in the project")
      }
    }
  }

  @ViewBuilder private func testRow(_ channel: NotifyChannel) -> some View {
    let name = NotificationsText.name(channel)
    LabeledContent {
      Button("Send Test Message") { Task { await test(channel) } }
        .disabled(testing != nil)
        .accessibilityLabel(Text("Send Test Message to \(name)"))
    } label: {
      if testing == channel {
        HStack {
          ProgressView().controlSize(.small)
          Text("Sending…")
        }
      } else if let tested, tested.channel == channel {
        if let problem = tested.problem {
          Label {
            Text(problem).textSelection(.enabled)
          } icon: {
            Image(systemName: "xmark.octagon.fill").foregroundStyle(.red)
          }
        } else {
          Label {
            Text("Sent. Check \(name) for it.")
          } icon: {
            Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
          }
        }
      } else {
        Text("Saves the settings and sends a short message")
      }
    }
  }

  /// Lists the chats the bot has been written in, to pick the one to post to.
  @ViewBuilder private var chatFinder: some View {
    LabeledContent {
      Button("Find Chat") { Task { await findChats() } }
        .disabled(chatSearch == .searching)
        .accessibilityHint(Text("Looks for the chats the bot has been written in"))
    } label: {
      switch chatSearch {
      case .idle:
        Text("Write to the bot in Telegram, then find its chat here instead of typing the ID.")
      case .searching:
        HStack {
          ProgressView().controlSize(.small)
          Text("Looking…")
        }
      case .failed(let problem):
        Label {
          Text(problem).textSelection(.enabled)
        } icon: {
          Image(systemName: "xmark.octagon.fill").foregroundStyle(.red)
        }
      case .found(let bot, let chats) where chats.isEmpty:
        Label {
          Text(NotificationsText.noChats(bot: bot)).textSelection(.enabled)
        } icon: {
          Image(systemName: "questionmark.circle.fill").foregroundStyle(.orange)
        }
      case .found(let bot, _):
        Text("Chats \(bot) has seen, the latest first. Choose where the messages go.")
      }
    }
    if case .found(_, let chats) = chatSearch {
      ForEach(chats, id: \.id) { chat in
        let chosen = draft.chat.trimmingCharacters(in: .whitespacesAndNewlines) == chat.id
        LabeledContent {
          Button("Use") { draft.chat = chat.id }
            .disabled(chosen)
            .accessibilityLabel(Text("Use \(chat.title)"))
        } label: {
          Label {
            Text(chat.title)
            Text("\(NotificationsText.kind(chat.kind)) · ID \(chat.id)")
          } icon: {
            Image(systemName: chosen ? "checkmark.circle.fill" : NotificationsText.symbol(chat.kind))
              .foregroundStyle(chosen ? .green : .secondary)
          }
        }
      }
    }
  }

  private func secretName(_ channel: NotifyChannel) -> String {
    switch channel {
    case .slack: String(localized: "Slack webhook")
    case .telegram: String(localized: "Telegram bot token")
    case .email: String(localized: "mail password")
    }
  }

  private func secretTitle(_ channel: NotifyChannel) -> Text {
    switch channel {
    case .slack: Text("Slack Webhook")
    case .telegram: Text("Telegram Bot Token")
    case .email: Text("Mail Password")
    }
  }

  private func secretMessage(_ channel: NotifyChannel) -> Text {
    let name = NotificationsText.name(channel)
    return Text("It’s kept in the Keychain on this Mac for this project and sent only to \(name). JEVision never shows it.")
  }

  private func load(_ root: URL) {
    do {
      let settings = try notificationSettings(project: root.path)
      saved = settings
      diagnostics = notificationSettingsIssues(project: root.path)
      draft = NotificationsDraft(settings)
      chatSearch = .idle
      loadProblem = nil
    } catch {
      saved = nil
      diagnostics = []
      loadProblem = NotificationsText.message(error)
    }
  }

  @discardableResult
  private func save(_ root: URL) -> Bool {
    switch draft.settings() {
    case .failure(let problem):
      failure = problem.message
      return false
    case .success(let settings):
      do {
        try saveNotificationSettings(project: root.path, settings: settings)
        diagnostics = notificationSettingsIssues(project: root.path)
        saved = settings
        draft = NotificationsDraft(settings)
        return true
      } catch {
        failure = NotificationsText.message(error)
        return false
      }
    }
  }

  private func findChats() async {
    guard let root = project.rootURL else { return }
    chatSearch = .searching
    let path = root.path
    let result = await Task.detached { Result { try findTelegramChats(project: path) } }.value
    chatSearch = NotificationsText.chatSearch(result)
  }

  private func test(_ channel: NotifyChannel) async {
    guard let root = project.rootURL, !hasChanges || save(root) else { return }
    testing = channel
    tested = nil
    let path = root.path
    let result = await Task.detached { Result { try sendTestNotification(project: path, channel: channel) } }.value
    testing = nil
    switch result {
    case .success: tested = (channel, nil)
    case .failure(let error): tested = (channel, NotificationsText.message(error))
    }
  }
}

extension NotificationSettings {
  /// Every channel off: what a project without the settings file has.
  static let off = NotificationSettings(unattendedOnly: false, slack: nil, telegram: nil, email: nil)
}

extension NotifyChannel: @retroactive Identifiable {
  public var id: Self { self }
}
