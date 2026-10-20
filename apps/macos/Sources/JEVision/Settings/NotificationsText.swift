import Foundation
import JEVisionCore

/// The notification settings as the form edits them: text as typed, and the channels that are
/// off keep what was typed for them.
struct NotificationsDraft: Equatable {
  var unattendedOnly = false
  var slack = false
  var slackWhen = NotifyWhen.failedOrFixed
  var telegram = false
  var telegramWhen = NotifyWhen.failedOrFixed
  var chat = ""
  var email = false
  var emailWhen = NotifyWhen.failedOrFixed
  var server = ""
  var port = "465"
  var security = MailSecurity.tls
  var user = ""
  var from = ""
  /// Addresses separated by commas or new lines.
  var to = ""

  init() {}

  init(_ settings: NotificationSettings) {
    unattendedOnly = settings.unattendedOnly
    if let slack = settings.slack {
      self.slack = true
      slackWhen = slack.when
    }
    if let telegram = settings.telegram {
      self.telegram = true
      telegramWhen = telegram.when
      chat = telegram.chat
    }
    if let mail = settings.email {
      email = true
      emailWhen = mail.when
      server = mail.server
      port = String(mail.port)
      security = mail.security
      user = mail.user
      from = mail.from
      to = mail.to.joined(separator: ", ")
    }
  }

  var addresses: [String] {
    to.split(whereSeparator: { $0 == "," || $0 == ";" || $0.isNewline })
      .map { $0.trimmingCharacters(in: .whitespaces) }
      .filter { !$0.isEmpty }
  }

  /// The settings to save, or what to fix first.
  func settings() -> Result<NotificationSettings, NotificationsProblem> {
    let trim = { (text: String) in text.trimmingCharacters(in: .whitespacesAndNewlines) }
    if telegram, trim(chat).isEmpty { return .failure(.noChat) }
    var mail: MailServer?
    if email {
      guard !trim(server).isEmpty else { return .failure(.noServer) }
      guard let port = UInt16(trim(self.port)), port > 0 else { return .failure(.badPort(self.port)) }
      guard Self.looksLikeAddress(from) else { return .failure(.badSender(from)) }
      guard !addresses.isEmpty else { return .failure(.noRecipients) }
      if let bad = addresses.first(where: { !Self.looksLikeAddress($0) }) { return .failure(.badRecipient(bad)) }
      mail = MailServer(
        server: trim(server), port: port, security: security, user: trim(user), from: trim(from), to: addresses,
        when: emailWhen)
    }
    return .success(
      NotificationSettings(
        unattendedOnly: unattendedOnly, slack: slack ? SlackChannel(when: slackWhen) : nil,
        telegram: telegram ? TelegramChat(chat: trim(chat), when: telegramWhen) : nil, email: mail))
  }

  /// `name@host` or `Name <name@host>`; the core checks the address in full.
  static func looksLikeAddress(_ text: String) -> Bool {
    var address = text.trimmingCharacters(in: .whitespaces)
    if address.hasSuffix(">"), let open = address.lastIndex(of: "<") {
      address = String(address[address.index(after: open)..<address.index(before: address.endIndex)])
    }
    let parts = address.split(separator: "@", omittingEmptySubsequences: false)
    return parts.count == 2 && !parts[0].isEmpty && parts[1].contains(".") && !address.contains(" ")
  }
}

/// What to fix in the form before the settings can be saved.
enum NotificationsProblem: Error, Equatable {
  case noChat, noServer, badPort(String), badSender(String), noRecipients, badRecipient(String)

  var message: String {
    switch self {
    case .noChat: String(localized: "Type the ID of the Telegram chat to post to.")
    case .noServer: String(localized: "Type the address of the mail server, such as smtp.gmail.com.")
    case .badPort(let port): String(localized: "“\(port)” isn’t a port. Mail servers usually use 465 or 587.")
    case .badSender(let address): String(localized: "“\(address)” isn’t an email address to send from.")
    case .noRecipients: String(localized: "Type at least one address to send the mail to.")
    case .badRecipient(let address): String(localized: "“\(address)” isn’t an email address to send to.")
    }
  }
}

/// Where looking for the Telegram chat to post to stands.
enum ChatSearch: Equatable {
  case idle
  case searching
  /// The bot's address and the chats it has seen; none when nobody wrote to it.
  case found(bot: String, chats: [FoundChat])
  case failed(String)
}

enum NotificationsText {
  static func name(_ channel: NotifyChannel) -> String {
    switch channel {
    case .slack: "Slack"
    case .telegram: "Telegram"
    case .email: String(localized: "Email")
    }
  }

  /// Why messages couldn't be sent or the settings couldn't be read or saved.
  static func message(_ error: any Error) -> String {
    guard let error = error as? NotifyError else { return error.localizedDescription }
    return switch error {
    case .Unreadable(let file, let reason): String(localized: "\(file) can’t be read: \(reason)")
    case .Unwritable(let file, let reason): String(localized: "\(file) can’t be saved: \(reason)")
    case .Rejected(let reason): reason
    case .NotSetUp(let channel): String(localized: "\(name(channel)) is off in this project’s notification settings.")
    case .NoSecret(.slack): String(localized: "Set the Slack webhook first.")
    case .NoSecret(.telegram): String(localized: "Set the Telegram bot token first.")
    case .NoSecret(.email): String(localized: "Set the mail password first.")
    case .Failed(let reason): reason
    }
  }

  /// What each choice of when to send a message means, in a sentence under the picker.
  static func explanation(_ when: NotifyWhen) -> String {
    switch when {
    case .failed: String(localized: "A message for every test that fails.")
    case .failedOrFixed: String(localized: "A message for every test that fails, and when a failing test passes again.")
    case .changed:
      String(localized: "A message when a test starts failing and when it passes again. A test that keeps failing says nothing more.")
    case .every: String(localized: "A message after every run, passed or failed.")
    }
  }

  /// What went wrong with the messages after a run, or `nil` when every one was sent.
  static func problem(_ sent: [NotifySent]) -> String? {
    let failures = sent.compactMap { sent in
      sent.failure.map { String(localized: "No message to \(name(sent.channel)): \(message($0))") }
    }
    return failures.isEmpty ? nil : failures.joined(separator: "\n")
  }

  /// Sends the message the project's settings ask for about the runs in `dirs`, one batch: one
  /// message about a single run, one summary about several. What went wrong, if anything. Waits
  /// for the services to answer.
  static func notify(project: String, dirs: [String]) -> String? {
    do {
      return problem(try notifyRuns(project: project, dirs: dirs))
    } catch {
      return dirs.count == 1
        ? String(localized: "No messages about the run were sent: \(message(error))")
        : String(localized: "No messages about the runs were sent: \(message(error))")
    }
  }

  static func chatSearch(_ result: Result<FoundChats, any Error>) -> ChatSearch {
    switch result {
    case .success(let found): .found(bot: found.bot, chats: found.chats)
    case .failure(let error): .failed(message(error))
    }
  }

  static func kind(_ kind: ChatKind) -> String {
    switch kind {
    case .person: String(localized: "Private chat")
    case .group: String(localized: "Group")
    case .channel: String(localized: "Channel")
    }
  }

  static func symbol(_ kind: ChatKind) -> String {
    switch kind {
    case .person: "person.fill"
    case .group: "person.3.fill"
    case .channel: "megaphone.fill"
    }
  }

  /// What to tell when no chat was found: the bot has to be written to first.
  static func noChats(bot: String) -> String {
    String(
      localized:
        "\(bot) hasn’t seen any chat yet. In Telegram, write something to \(bot). To use a group, add \(bot) to it and send a message there. Then find the chat again."
    )
  }
}
