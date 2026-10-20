import JEVisionCore
import JEVisionHands
import SwiftUI

/// Under the report of a failed run: the AI says what most likely went wrong and offers the fix
/// that fits, made in one click through what the app already does.
struct FailureAnalysisCard: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.undoManager) private var undoManager
  @Environment(\.openSettings) private var openSettings
  let run: SavedRun
  let runAgain: () -> Void
  let openFailedBlock: (Step) -> Void
  @State private var conversation = AIConversation()
  @State private var explanation: FailureExplanation?
  @State private var noModel = false
  /// The model of the last analysis reads only text.
  @State private var textOnly = false
  @State private var fixed = false
  @State private var fixError: String?
  @State private var quarantining: QuarantineRequest?

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      switch conversation.phase {
      case .working:
        HStack(spacing: 10) {
          ProgressView().controlSize(.small)
          Text("The AI is looking at the run…")
          Spacer()
          Button("Stop") { conversation.stop() }
        }
      case .approving(let call):
        ToolApproval(call: call) { conversation.decide(allow: $0) }
      case .idle, .failed:
        if let explanation {
          result(explanation)
        } else {
          offer
        }
      }
    }
    .padding(18)
    .background(.background, in: .rect(cornerRadius: 18))
    .overlay(RoundedRectangle(cornerRadius: 18).strokeBorder(.separator))
    .quarantineSheet($quarantining)
    .onDisappear { conversation.stop() }
  }

  private var offer: some View {
    Group {
      HStack(alignment: .firstTextBaseline) {
        VStack(alignment: .leading, spacing: 4) {
          Label("What went wrong?", systemImage: "sparkles")
            .font(.headline)
            .accessibilityAddTraits(.isHeader)
          Text("The AI reads the steps and screenshots of this run and says whether it’s a bug in the app, a flaky test, an outdated reference or a problem with this Mac.")
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        }
        Spacer()
        Button("Analyze", systemImage: "sparkles") { analyze() }
          .buttonStyle(.glass)
          .help(Text("Ask the AI why the run failed"))
      }
      if noModel {
        HStack {
          Label("Choose an AI model in Settings → AI first.", systemImage: "exclamationmark.triangle.fill")
            .foregroundStyle(.orange)
          Spacer()
          Button("Open Settings") { openSettings() }
        }
      } else if case .failed(let message) = conversation.phase {
        Label(message, systemImage: "exclamationmark.triangle.fill")
          .foregroundStyle(.red)
          .textSelection(.enabled)
          .fixedSize(horizontal: false, vertical: true)
      }
    }
  }

  private func result(_ explanation: FailureExplanation) -> some View {
    Group {
      Label {
        Text(FailureText.cause(explanation.cause))
      } icon: {
        Image(systemName: FailureText.symbol(explanation.cause)).foregroundStyle(.purple)
      }
      .font(.headline)
      .accessibilityAddTraits(.isHeader)
      Text(verbatim: explanation.explanation)
        .textSelection(.enabled)
        .fixedSize(horizontal: false, vertical: true)
      Group {
        if explanation.picturesWithheld, textOnly {
          Text("Screenshots weren’t sent: this model reads only text. The AI judged from the steps alone.")
        } else if explanation.picturesWithheld {
          Text("Screenshots weren’t sent: only models on this Mac may see them. The AI judged from the steps alone.")
        } else if explanation.picturesSent > 0 {
          Text("The AI saw \(explanation.picturesSent) screenshots of the run.")
        }
      }
      .font(.callout)
      .foregroundStyle(.secondary)
      .fixedSize(horizontal: false, vertical: true)
      AIToolsUsed(calls: explanation.calls, unavailable: explanation.unavailable)
      if let fixError {
        Text(fixError).foregroundStyle(.red).font(.callout)
      }
      HStack(spacing: 8) {
        if fixed {
          Label("Done. Undo brings it back.", systemImage: "checkmark.circle.fill")
            .foregroundStyle(.green)
        } else {
          Button(FailureText.fix(explanation.fix), systemImage: FailureText.symbol(explanation.fix)) { fix(explanation) }
            .buttonStyle(.glassProminent)
            .disabled(!canFix(explanation))
        }
        Button("Analyze Again") { analyze() }
          .buttonStyle(.glass)
      }
      .padding(.top, 4)
    }
  }

  private func analyze() {
    let ai = AISettings.app
    guard let settings = ai.textSettings(project: project.settings) else {
      noModel = true
      return
    }
    noModel = false
    guard let root = project.rootURL else { return }
    let report = run.report
    let request = FailureRequest(
      run: run.url.path(percentEncoded: false), project: root.path(percentEncoded: false),
      localOnly: AISettings.app.localOnly || project.settings.localOnly, flaky: project.flaky[report.file])
    let servers = MCPServers.all
    let language = Self.language
    let safeguards = ai.safeguards(for: settings)
    textOnly = !settings.pictures
    conversation.start {
      try AiSession.explainFailure(
        settings: settings, servers: servers, request: request, language: language, safeguards: safeguards)
    } finished: { step in
      guard case .explained(let explanation) = step else { return }
      self.explanation = explanation
      fixed = false
      fixError = nil
      AccessibilityNotification.Announcement(
        FailureText.cause(explanation.cause) + ". " + explanation.explanation
      ).post()
    }
  }

  /// The language the app speaks, by its English name, for the AI to answer in.
  private static var language: String {
    let code = AppLanguage.shared.language
    return Locale(identifier: "en").localizedString(forLanguageCode: code) ?? "English"
  }

  private func canFix(_ explanation: FailureExplanation) -> Bool {
    switch explanation.fix {
    case .runAgain, .addRetries, .quarantine, .addWait: project.hasTest(explanation.test)
    case .updateReference: RunHistory.failedStep(run.report)?.after != nil
    case .openBlock: RunHistory.failedStep(run.report).map { project.hasTest($0.test) } ?? false
    }
  }

  private func fix(_ explanation: FailureExplanation) {
    let failed = RunHistory.failedStep(run.report)
    fixError = nil
    switch explanation.fix {
    case .updateReference:
      guard let after = failed?.after, let reference = failed?.reference else { return }
      do {
        try project.acceptReference(run.url.appending(path: after), as: reference, undoManager: undoManager)
        done()
      } catch {
        fixError = String(localized: "Can’t replace the reference image: \(error.localizedDescription)")
      }
    case .addWait:
      guard let block = explanation.block else { return }
      project.showBlock(block, in: explanation.test, from: run.id)
      guard let document = project.document, let flow = document.flow,
        let edited = addWaitBefore(flow: flow, block: block)
      else { return }
      if document.undoManager == nil { document.undoManager = undoManager }
      document.apply(edited, action: String(localized: "Add Wait"))
    case .addRetries:
      project.sidebar = .tests
      project.selection = explanation.test
      // The core offers this fix while a failed test is retried fewer than two times.
      project.document?.updateFlow(field: "retries") { $0.retries = 2 }
    case .quarantine:
      quarantining = QuarantineRequest(
        file: explanation.test, name: run.report.test, entry: project.quarantine(of: explanation.test))
    case .runAgain:
      runAgain()
    case .openBlock:
      if let failed { openFailedBlock(failed) }
    }
  }

  private func done() {
    fixed = true
    AccessibilityNotification.Announcement(String(localized: "Done. Undo brings it back.")).post()
  }
}

/// Causes of a failure and their fixes, in words.
enum FailureText {
  static func cause(_ cause: FailureCause) -> String {
    switch cause {
    case .appBug: String(localized: "Looks like a bug in the app")
    case .flakyTest: String(localized: "Looks like a flaky test")
    case .outdatedReference: String(localized: "The reference image looks out of date")
    case .environment: String(localized: "Looks like a problem with this Mac, not the app")
    }
  }

  static func symbol(_ cause: FailureCause) -> String {
    switch cause {
    case .appBug: "ladybug"
    case .flakyTest: "shuffle"
    case .outdatedReference: "photo.on.rectangle"
    case .environment: "desktopcomputer"
    }
  }

  static func fix(_ fix: FailureFix) -> String {
    switch fix {
    case .updateReference: String(localized: "Use the Screenshot as the Reference")
    case .addWait: String(localized: "Add a Wait Before the Block")
    case .addRetries: String(localized: "Retry the Test Twice When It Fails")
    case .quarantine: String(localized: "Put the Test in Quarantine…")
    case .runAgain: String(localized: "Run Again")
    case .openBlock: String(localized: "Open the Block")
    }
  }

  @MainActor static func symbol(_ fix: FailureFix) -> String {
    switch fix {
    case .updateReference: "checkmark.seal"
    case .addWait: "hourglass"
    case .addRetries: "arrow.clockwise"
    case .quarantine: RunStatus.quarantined.symbol
    case .runAgain: "arrow.clockwise"
    case .openBlock: "scope"
    }
  }
}
