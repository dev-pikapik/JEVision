import JEVisionCore
import JEVisionHands
import SwiftUI

/// A conversation with the AI model about one task, such as writing a test. It talks to the model
/// off the main thread, stops while the person decides on a tool call that may change something,
/// and can be stopped at any time.
@MainActor
@Observable
final class AIConversation {
  enum Phase: Equatable {
    case idle
    case working
    /// The model wants to use a tool that may change something.
    case approving(AiToolCall)
    case failed(String)
  }

  private(set) var phase = Phase.idle
  @ObservationIgnored private var session: AiSession?
  /// Counts conversations, so an answer to one that was stopped is dropped.
  @ObservationIgnored private var generation = 0
  @ObservationIgnored private var finished: (AiStep) -> Void = { _ in }

  var isBusy: Bool {
    switch phase {
    case .working, .approving: true
    case .idle, .failed: false
    }
  }

  /// Starts a conversation made by `make`; `finished` gets its result.
  func start(_ make: @escaping @Sendable () throws -> AiSession, finished: @escaping (AiStep) -> Void) {
    stop()
    self.finished = finished
    phase = .working
    let current = generation
    Task {
      let made = await Task.detached { Result { try make() } }.value
      guard current == generation else { return }
      switch made {
      case .success(let session):
        self.session = session
        advance { try session.step() }
      case .failure(let error):
        fail(error)
      }
    }
  }

  /// Lets the tool call asked about run, or tells the model it may not.
  func decide(allow: Bool) {
    guard case .approving = phase, let session else { return }
    phase = .working
    advance { try session.decide(allow: allow) }
  }

  func stop() {
    session?.cancel()
    session = nil
    generation += 1
    phase = .idle
  }

  private func advance(_ work: @escaping @Sendable () throws -> AiStep) {
    let current = generation
    Task {
      let result = await Task.detached { Result { try work() } }.value
      guard current == generation else { return }
      switch result {
      case .success(.approve(let call)):
        phase = .approving(call)
        AccessibilityNotification.Announcement(AIText.asks(call)).post()
      case .success(let step):
        session = nil
        phase = .idle
        finished(step)
      case .failure(let error):
        fail(error)
      }
    }
  }

  private func fail(_ error: Error) {
    session = nil
    if case AiError.Cancelled = error {
      phase = .idle
      return
    }
    let message = AIText.error(error)
    phase = .failed(message)
    AccessibilityNotification.Announcement(message).post()
  }
}

/// What the AI features say, in words.
@MainActor
enum AIText {
  static func error(_ error: Error) -> String {
    switch error {
    case AiError.UnknownProvider(let provider):
      String(localized: "There’s no AI provider “\(provider)”. Choose one in Settings → AI.")
    case AiError.OnlyChecks(let provider):
      String(localized: "\(provider) only checks screenshots. To write tests and analyze failures, choose another provider in Settings → AI.")
    case AiError.Provider(let failure, _) where failure.code == "run.vision.refused":
      String(localized: "\(failure.params["provider"] ?? "") declined to help: \(failure.params["message"] ?? "")")
    case AiError.Provider(let failure, _):
      VisionText.failure(failure)
    case AiError.NoReport(let reason):
      String(localized: "Can’t read the report of the run: \(reason)")
    case AiError.Unusable(let problems):
      String(localized: "The AI’s answer still had mistakes after it was asked to fix them. Try describing it differently.\n\(problems)")
    case AiError.TooManyCalls(let limit):
      String(localized: "The AI was stopped after \(limit) tool calls without an answer.")
    case let error as McpError:
      mcp(error)
    default:
      error.localizedDescription
    }
  }

  static func mcp(_ error: McpError) -> String {
    switch error {
    case .NoCommand: String(localized: "The command is empty.")
    case .BadAddress(let url): String(localized: "“\(url)” isn’t a web address. It should start with http:// or https://.")
    case .CannotStart(let command, let reason): String(localized: "Can’t start “\(command)”: \(reason)")
    case .Stopped(let output):
      output.isEmpty
        ? String(localized: "The server stopped.") : String(localized: "The server stopped. It said: \(output)")
    case .Unreachable(let url): String(localized: "Can’t reach \(url). Check that the server is running.")
    case .Unauthorized: String(localized: "The server didn’t accept the access token. Set it again.")
    case .Timeout(let seconds): String(localized: "The server didn’t answer within \(seconds) s.")
    case .Http(let status, let message): String(localized: "The server answered with error \(status): \(message)")
    case .Rpc(let code, let message): String(localized: "The server answered with error \(code): \(message)")
    case .Protocol(let message): String(localized: "The server’s answer can’t be read: \(message)")
    }
  }

  /// A tool call the AI made, for the list of what it used.
  static func used(_ call: AiToolCall) -> String {
    call.allowed
      ? String(localized: "\(call.title) on \(call.server)") : String(localized: "\(call.title) on \(call.server), not allowed")
  }

  static func asks(_ call: AiToolCall) -> String {
    String(localized: "The AI wants to use \(call.title) on \(call.server). Allow it?")
  }
}

/// Asks to allow a tool call that may change something: what it is, with what, and two buttons.
struct ToolApproval: View {
  let call: AiToolCall
  let decide: (Bool) -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      Label {
        Text("Allow \(call.title) on \(call.server)?").font(.headline)
      } icon: {
        Image(systemName: "hand.raised.fill").foregroundStyle(.orange)
      }
      Text("This tool may change something outside JEVision. The AI uses it only if you allow it.")
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      if !["", "{}"].contains(call.arguments) {
        ScrollView {
          Text(verbatim: call.arguments)
            .font(.callout.monospaced())
            .textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(maxHeight: 120)
        .accessibilityLabel(Text("What the tool gets"))
      }
      HStack {
        Spacer()
        Button("Don’t Allow") { decide(false) }
          .keyboardShortcut(.cancelAction)
        // No default button: Return must not allow a change by accident.
        Button("Allow") { decide(true) }
      }
    }
  }
}

/// The tools the AI used and the servers it couldn't reach, under its answer.
struct AIToolsUsed: View {
  let calls: [AiToolCall]
  let unavailable: [McpProblem]

  var body: some View {
    if !calls.isEmpty {
      Label {
        Text("Used: \(calls.map(AIText.used).formatted(.list(type: .and)))")
      } icon: {
        Image(systemName: "wrench.and.screwdriver")
      }
      .font(.callout)
      .foregroundStyle(.secondary)
    }
    ForEach(unavailable, id: \.server) { problem in
      Label {
        Text("Couldn’t use \(problem.server): \(AIText.mcp(problem.error))")
      } icon: {
        Image(systemName: "exclamationmark.triangle").foregroundStyle(.orange)
      }
      .font(.callout)
      .fixedSize(horizontal: false, vertical: true)
    }
  }
}
