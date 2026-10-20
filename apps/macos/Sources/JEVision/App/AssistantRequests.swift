import AppKit
import JEVisionCore
import JEVisionHands

/// Asks the person at the Mac before an AI assistant, which started `jevision mcp`, runs a test.
/// The core decides and waits for the answer; this only shows the question, once a second while
/// the app is open. An alert nobody answers in time counts as a no.
@MainActor
final class AssistantRequests {
  private let folder: String
  private var timer: Timer?
  private var asking = false

  init(folder: String = AISettings.assistantsFolder.path) {
    self.folder = folder
  }

  func start() {
    timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
      MainActor.assumeIsolated { self?.check() }
    }
  }

  func check() {
    guard !asking, let request = pendingAssistantRequests(folder: folder).first else { return }
    asking = true
    defer { asking = false }
    let allow = Self.ask(request)
    _ = answerAssistantRequest(folder: folder, id: request.id, allow: allow)
  }

  /// Shows what the assistant wants to do; true when the person allows it.
  static func ask(_ request: AssistantRequest) -> Bool {
    let alert = NSAlert()
    alert.messageText = title(request)
    alert.informativeText = details(request)
    alert.addButton(withTitle: String(localized: "Allow"))
    alert.addButton(withTitle: String(localized: "Don’t Allow"))
    let wait = max(0, Date(timeIntervalSince1970: TimeInterval(request.expiresAt)).timeIntervalSinceNow)
    let timer = Timer(timeInterval: wait, repeats: false) { _ in
      MainActor.assumeIsolated { NSApp.abortModal() }
    }
    RunLoop.main.add(timer, forMode: .modalPanel)
    defer { timer.invalidate() }
    NSApp.activate()
    return alert.runModal() == .alertFirstButtonReturn
  }

  static func title(_ request: AssistantRequest) -> String {
    String(localized: "Allow “\(request.assistant)” to run \(request.tests.count) test?")
  }

  /// The tests, the project, the apps and what running does, in words anyone follows.
  static func details(_ request: AssistantRequest, appName: (String) -> String = { AppCatalog.shared.name(of: $0) }) -> String {
    let shown = request.tests.prefix(5).map { $0.replacing(/\.flow\.json$/, with: "") }
    var lines = shown
    if request.tests.count > shown.count {
      lines.append(String(localized: "…and \(request.tests.count - shown.count) more"))
    }
    lines.append("")
    lines.append(String(localized: "Project: \(URL(filePath: request.project).lastPathComponent)"))
    if !request.apps.isEmpty {
      lines.append(String(localized: "Apps: \(request.apps.map(appName).formatted(.list(type: .and)))"))
    }
    if let environment = request.environment {
      lines.append(String(localized: "Environment: \(environment)"))
    }
    lines.append("")
    lines.append(String(localized: "It will use the mouse and keyboard of this Mac until it ends. Press Esc twice to stop it. Without an answer in a minute, it isn’t run."))
    return lines.joined(separator: "\n")
  }
}
