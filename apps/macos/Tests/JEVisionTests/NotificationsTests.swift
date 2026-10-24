import Foundation
import JEVisionCore
import Testing

@testable import JEVision

struct NotificationsTests {
  @Test func savesRetainCurrentOptionalFieldsThroughTheFrontendRecord() throws {
    let project = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: project, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: project) }
    let file = project.appending(path: notificationSettingsFileName())
    try #"{"format":2,"minVersion":"0.1.0","extra":{"keep":42},"slack":{"when":"changed","extra":[1,2]}}"#
      .write(to: file, atomically: true, encoding: .utf8)
    var stale = try notificationSettings(project: project.path)
    stale.slack = SlackChannel(when: .every)
    try #"{"format":2,"minVersion":"0.1.0","latest":true,"slack":{"when":"changed","extra":[3,4]}}"#
      .write(to: file, atomically: true, encoding: .utf8)
    try saveNotificationSettings(project: project.path, settings: stale)
    let saved = try #require(JSONSerialization.jsonObject(with: Data(contentsOf: file)) as? [String: Any])
    #expect(saved["extra"] == nil)
    #expect(saved["latest"] as? Bool == true)
    let slack = try #require(saved["slack"] as? [String: Any])
    #expect(slack["extra"] as? [Int] == [3, 4])
    #expect(slack["when"] as? String == "every")
    #expect(notificationSettingsIssues(project: project.path).map(\.code) == ["settings.field.unknown", "settings.field.unknown"])
    try saveNotificationSettings(project: project.path, settings: .off)
    let off = try #require(JSONSerialization.jsonObject(with: Data(contentsOf: file)) as? [String: Any])
    #expect(off["slack"] == nil)
    #expect(off["latest"] as? Bool == true)
    #expect(off["minVersion"] as? String == "0.1.0")
  }

  @Test func unsupportedOrBrokenFilesRefuseStaleSaveAndClear() throws {
    let project = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: project, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: project) }
    let file = project.appending(path: notificationSettingsFileName())
    var stale = NotificationSettings.off
    stale.slack = SlackChannel(when: .every)
    for (text, code) in [
      (#"{"format":99,"slack":true}"#, "settings.format.tooNew"),
      (#"{"format":2,"minVersion":"9999.0.0","slack":true}"#, "settings.needsNewer"),
      (#"{"format":2,"minVersion":null}"#, "settings.minVersion.invalid"),
      (#"{"format":2,"slack":{},"slack":{}}"#, "settings.notification.invalid"),
      (#"{"format":2,"optional":{"password":"hidden-value"}}"#, "settings.notification.invalid"),
      ("{", "settings.notification.invalid"),
    ] {
      try text.write(to: file, atomically: true, encoding: .utf8)
      #expect(throws: NotifyError.self) { try saveNotificationSettings(project: project.path, settings: stale) }
      #expect(throws: NotifyError.self) { try saveNotificationSettings(project: project.path, settings: .off) }
      #expect(throws: NotifyError.self) { try notificationSettings(project: project.path) }
      #expect(try String(contentsOf: file, encoding: .utf8) == text)
      let issues = notificationSettingsIssues(project: project.path)
      #expect(issues.map(\.code) == [code])
      #expect(!issues.map { describeIssue(issue: $0) }.joined().contains("hidden-value"))
    }
  }

  @Test func theDraftKeepsWhatWasTypedAndSaysWhatToFix() {
    var draft = NotificationsDraft()
    draft.telegram = true
    #expect(draft.settings() == .failure(.noChat))
    draft.chat = " -100123 "
    draft.telegramWhen = .changed
    draft.slack = true
    draft.slackWhen = .failed
    draft.email = true
    draft.emailWhen = .every
    draft.server = "smtp.acme.com"
    draft.port = "port"
    #expect(draft.settings() == .failure(.badPort("port")))
    draft.port = "587"
    draft.from = "JEVision <qa@acme.com>"
    draft.to = "team@acme.com;\nann"
    #expect(draft.settings() == .failure(.badRecipient("ann")))
    draft.to = "team@acme.com;\nann@acme.com, "
    let settings = try? draft.settings().get()
    #expect(settings?.slack == SlackChannel(when: .failed))
    #expect(settings?.telegram == TelegramChat(chat: "-100123", when: .changed))
    #expect(settings?.email?.when == .every)
    #expect(settings?.email?.port == 587)
    #expect(settings?.email?.to == ["team@acme.com", "ann@acme.com"])
    #expect(NotificationsDraft(settings!).settings() == .success(settings!))

    draft.email = false
    #expect((try? draft.settings().get())?.email == nil)
    #expect(draft.server == "smtp.acme.com")
    #expect(draft.emailWhen == .every)
  }

  @Test func eachChoiceOfWhenHasItsOwnExplanation() {
    let whens: [NotifyWhen] = [.failed, .failedOrFixed, .changed, .every]
    #expect(Set(whens.map(NotificationsText.explanation)).count == whens.count)
  }

  @Test func theChatSearchSaysWhatFoundNothingOrNoTokenMeans() {
    let chat = FoundChat(id: "-1001", title: "QA team", kind: .group)
    #expect(
      NotificationsText.chatSearch(.success(FoundChats(bot: "@acme_bot", chats: [chat])))
        == .found(bot: "@acme_bot", chats: [chat]))
    #expect(
      NotificationsText.chatSearch(.failure(NotifyError.Failed(reason: "Telegram can’t be reached")))
        == .failed("Telegram can’t be reached"))
    let none = NotificationsText.noChats(bot: "@acme_bot")
    #expect(none.contains("@acme_bot") && none.contains("add @acme_bot to it"))
    #expect(NotificationsText.kind(.group) != NotificationsText.kind(.channel))
    #expect(NotificationsText.symbol(.person) != NotificationsText.symbol(.group))

    let project = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    guard ProcessInfo.processInfo.environment[notificationSecretVariable(channel: .telegram)] == nil else { return }
    #expect(
      NotificationsText.chatSearch(Result { try findTelegramChats(project: project.path) })
        == .failed("Set the Telegram bot token first."))
  }

  @Test func aRunWithoutTheSecretSaysWhatIsMissing() throws {
    let project = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: project) }
    let dir = project.appending(path: ".jevision/runs/20261019-093000-sign-in")
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let report = """
      {"format": 1, "test": "Sign in", "file": "sign-in.flow.json", "app": "com.acme.mail",
       "startedAt": "2026-10-19T09:30:00Z", "durationMs": 1200, "verdict": "failed", "flaky": false,
       "attempts": [{"number": 1, "verdict": "failed", "durationMs": 1200, "steps": []}]}
      """
    try report.write(to: dir.appending(path: "report.json"), atomically: true, encoding: .utf8)
    #expect(NotificationsText.notify(project: project.path, dirs: [dir.path]) == nil)

    var settings = NotificationSettings.off
    settings.slack = SlackChannel(when: .failedOrFixed)
    try saveNotificationSettings(project: project.path, settings: settings)
    #expect(try notificationSettings(project: project.path) == settings)
    guard ProcessInfo.processInfo.environment[notificationSecretVariable(channel: .slack)] == nil else { return }
    #expect(
      NotificationsText.notify(project: project.path, dirs: [dir.path])
        == "No message to Slack: Set the Slack webhook first.")

    // Several runs are one batch: one summary, so one answer per channel, not one per run.
    let second = project.appending(path: ".jevision/runs/20261019-094000-sign-up")
    try FileManager.default.createDirectory(at: second, withIntermediateDirectories: true)
    try report.write(to: second.appending(path: "report.json"), atomically: true, encoding: .utf8)
    #expect(
      NotificationsText.notify(project: project.path, dirs: [dir.path, second.path])
        == "No message to Slack: Set the Slack webhook first.")
    #expect(
      NotificationsText.notify(project: project.path, dirs: [dir.path, project.appending(path: "gone").path])
        .map { $0.hasPrefix("No messages about the runs were sent: ") } == true)
  }
}
