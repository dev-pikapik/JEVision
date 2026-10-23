import AppKit
import JEVisionCore
import JEVisionHands

/// Asks the person at the Mac before an AI assistant, which started `jevision mcp`, runs a test.
/// The core decides and waits for the answer; this only shows the question, as soon as it comes
/// in while the app is open. An alert nobody answers in time counts as a no.
@MainActor
final class AssistantRequests {
  private let folder: String
  private let ask: @MainActor (AssistantRequest) -> Bool
  private var listener: AssistantListener?
  private var watch: DispatchSourceFileSystemObject?
  private var asking = false

  init(
    folder: String = AISettings.assistantsFolder.path,
    ask: @escaping @MainActor (AssistantRequest) -> Bool = AssistantRequests.ask
  ) {
    self.folder = folder
    self.ask = ask
  }

  /// Tells the servers the app can ask, and watches for their questions. Nothing waits on a
  /// timer: macOS slows the timers of an app it lets nap in the background, and the lock and
  /// the folder's changes reach a napping app all the same.
  func start() {
    guard listener == nil, let listener = listenForAssistants(folder: folder) else { return }
    self.listener = listener
    let asks = open(URL(filePath: folder).appending(path: "asks").path, O_EVTONLY)
    if asks >= 0 {
      let watch = DispatchSource.makeFileSystemObjectSource(
        fileDescriptor: asks, eventMask: [.write, .delete, .rename], queue: .main)
      watch.setEventHandler { [weak self] in
        MainActor.assumeIsolated {
          guard let self, let watch = self.watch else { return }
          // The folder itself went away: listen again in a new one.
          if watch.data.isDisjoint(with: [.delete, .rename]) { self.check() } else { self.stop(); self.start() }
        }
      }
      watch.setCancelHandler { close(asks) }
      watch.resume()
      self.watch = watch
    }
    check()
  }

  func stop() {
    watch?.cancel()
    watch = nil
    listener?.stop()
    listener = nil
  }

  /// Asks each question waiting, including ones that come in while an earlier one is shown.
  func check() {
    guard !asking else { return }
    asking = true
    defer { asking = false }
    var asked = Set<String>()
    while let request = pendingAssistantRequests(folder: folder).first(where: { !asked.contains($0.id) }) {
      asked.insert(request.id)
      _ = answerAssistantRequest(folder: folder, id: request.id, allow: ask(request))
    }
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
    // macOS may keep a background app from coming forward (an assistant opens JEVision without
    // bringing it to the front); the Dock icon then bounces until the person looks.
    let attention = NSApp.isActive ? nil : NSApp.requestUserAttention(.criticalRequest)
    defer { if let attention { NSApp.cancelUserAttentionRequest(attention) } }
    NSApp.activate()
    return alert.runModal() == .alertFirstButtonReturn
  }

  static func title(_ request: AssistantRequest) -> String {
    String(localized: "Allow “\(request.assistant)” to run \(request.tests.count) test?")
  }

  /// A test by the name it gives itself; by its file when that can't be read or has no name.
  static func testName(_ file: String, in project: String) -> String {
    let text = try? String(contentsOf: URL(filePath: project).appending(path: file), encoding: .utf8)
    let name = text.flatMap { try? loadFlow(json: $0) }?.name.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
    return name.isEmpty ? file.replacing(/\.flow\.json$/, with: "") : name
  }

  /// The tests, the project, the apps and what running does, in words anyone follows.
  static func details(_ request: AssistantRequest, appName: (String) -> String = { AppCatalog.shared.name(of: $0) }) -> String {
    let shown = request.tests.prefix(5).map { testName($0, in: request.project) }
    var lines = shown
    if request.tests.count > shown.count {
      lines.append(String(localized: "…and \(request.tests.count - shown.count) more"))
    }
    lines.append("")
    if let computer = request.remote {
      lines.append(String(localized: "Over the network, from “\(computer)”"))
    }
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
