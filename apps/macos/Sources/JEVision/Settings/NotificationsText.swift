import Foundation
import JEVisionCore

/// The notification settings as the form edits them: text as typed, and the channels that are
/// off keep what was typed for them.
struct NotificationsDraft: Equatable {
  var when = NotifyWhen.failedOrFixed
  var unattendedOnly = false
  var slack = false
  var telegram = false
  var chat = ""
  var email = false
  var server = ""
  var port = "465"
  var security = MailSecurity.tls
  var user = ""
  var from = ""
  /// Addresses separated by commas or new lines.
  var to = ""

  init() {}

  init(_ settings: NotificationSettings) {
    when = settings.when
    unattendedOnly = settings.unattendedOnly
    slack = settings.slack
    if let telegram = settings.telegram {
      self.telegram = true
      chat = telegram.chat
    }
    if let mail = settings.email {
      email = true
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
        server: trim(server), port: port, security: security, user: trim(user), from: trim(from), to: addresses)
    }
    return .success(
      NotificationSettings(
        when: when, unattendedOnly: unattendedOnly, slack: slack,
        telegram: telegram ? TelegramChat(chat: trim(chat)) : nil, email: mail))
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

  /// What went wrong with the messages after a run, or `nil` when every one was sent.
  static func problem(_ sent: [NotifySent]) -> String? {
    let failures = sent.compactMap { sent in
      sent.failure.map { String(localized: "No message to \(name(sent.channel)): \(message($0))") }
    }
    return failures.isEmpty ? nil : failures.joined(separator: "\n")
  }

  /// Sends the messages the project's settings ask for about the run in `dir`; what went wrong,
  /// if anything. Waits for the services to answer.
  static func notify(project: String, dir: String) -> String? {
    do {
      return problem(try notifyRun(project: project, dir: dir))
    } catch {
      return String(localized: "No messages about the run were sent: \(message(error))")
    }
  }
}
