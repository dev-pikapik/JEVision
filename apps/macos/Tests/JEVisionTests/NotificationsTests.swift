import Foundation
import JEVisionCore
import Testing

@testable import JEVision

struct NotificationsTests {
  @Test func theDraftKeepsWhatWasTypedAndSaysWhatToFix() {
    var draft = NotificationsDraft()
    draft.telegram = true
    #expect(draft.settings() == .failure(.noChat))
    draft.chat = " -100123 "
    draft.email = true
    draft.server = "smtp.acme.com"
    draft.port = "port"
    #expect(draft.settings() == .failure(.badPort("port")))
    draft.port = "587"
    draft.from = "JEVision <qa@acme.com>"
    draft.to = "team@acme.com;\nann"
    #expect(draft.settings() == .failure(.badRecipient("ann")))
    draft.to = "team@acme.com;\nann@acme.com, "
    let settings = try? draft.settings().get()
    #expect(settings?.telegram == TelegramChat(chat: "-100123"))
    #expect(settings?.email?.port == 587)
    #expect(settings?.email?.to == ["team@acme.com", "ann@acme.com"])
    #expect(NotificationsDraft(settings!).settings() == .success(settings!))

    draft.email = false
    #expect((try? draft.settings().get())?.email == nil)
    #expect(draft.server == "smtp.acme.com")
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
    #expect(NotificationsText.notify(project: project.path, dir: dir.path) == nil)

    var settings = NotificationSettings.off
    settings.slack = true
    try saveNotificationSettings(project: project.path, settings: settings)
    #expect(try notificationSettings(project: project.path) == settings)
    guard ProcessInfo.processInfo.environment[notificationSecretVariable(channel: .slack)] == nil else { return }
    #expect(
      NotificationsText.notify(project: project.path, dir: dir.path)
        == "No message to Slack: Set the Slack webhook first.")
  }
}
