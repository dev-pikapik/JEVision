import JEVisionCore
import JEVisionHands
import SwiftUI

/// Writes a test from a description in words. The draft is shown on the canvas in place of the
/// test until it is accepted, reworked or dropped.
@MainActor
@Observable
final class TestDrafter {
  var description: String
  /// What to change in the draft.
  var feedback = ""
  let conversation = AIConversation()
  private(set) var draft: TestDraft?
  /// Why writing can't start.
  private(set) var blocked: Blocked?
  /// Writing starts as soon as the panel shows, for a test created with a description.
  @ObservationIgnored var startsRightAway: Bool

  enum Blocked { case noApp, noModel }

  init(description: String = "", startsRightAway: Bool = false) {
    self.description = description
    self.startsRightAway = startsRightAway
  }

  /// Asks the AI for a test of `document`; with `rework`, for the draft shown changed as
  /// `feedback` says.
  func write(_ document: FlowDocument, localOnly: Bool, project: ProjectSettings, rework: Bool = false) {
    guard let flow = document.beforeDraft ?? document.flow else { return }
    if flow.app.isEmpty {
      blocked = .noApp
      return
    }
    let ai = AISettings.app
    guard let settings = ai.textSettings(project: project) else {
      blocked = .noModel
      return
    }
    blocked = nil
    let request = DraftRequest(
      description: description, app: flow.app, name: flow.name, current: flow,
      folder: document.url.deletingLastPathComponent().path(percentEncoded: false),
      previous: rework && document.beforeDraft != nil ? document.flow : nil, feedback: rework ? feedback : "",
      localOnly: localOnly)
    let servers = MCPServers.all
    let safeguards = ai.safeguards(for: settings)
    conversation.start {
      try AiSession.draftTest(settings: settings, servers: servers, request: request, safeguards: safeguards)
    } finished: { [weak self, weak document] step in
      guard let self, let document, case .drafted(let draft) = step else { return }
      self.draft = draft
      feedback = ""
      document.showDraft(draft.flow)
      AccessibilityNotification.Announcement(
        String(localized: "The draft is on the canvas with \(draft.flow.blocks.count) blocks. Accept it, or say what to change.")
      ).post()
    }
  }

  /// The draft is no longer shown, as after Undo: back to the description.
  func dropDraft() {
    draft = nil
  }
}

/// The panel over the canvas for writing a test from a description: the description, the AI at
/// work, its questions about tools and the draft with Accept, Rework and Cancel.
struct DraftPanel: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.openSettings) private var openSettings
  let document: FlowDocument
  @Bindable var drafter: TestDrafter
  @FocusState private var focused: Bool

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      switch drafter.conversation.phase {
      case .working:
        working
      case .approving(let call):
        ToolApproval(call: call) { drafter.conversation.decide(allow: $0) }
      case .idle, .failed:
        if let draft = drafter.draft, document.beforeDraft != nil {
          result(draft)
        } else {
          describing
        }
      }
    }
    .padding(16)
    .frame(width: 440)
    .adaptiveGlass(in: .rect(cornerRadius: 20))
    .padding(.top, 12)
    .onAppear {
      focused = true
      if drafter.startsRightAway {
        drafter.startsRightAway = false
        write()
      }
    }
    .onChange(of: document.beforeDraft == nil) { _, gone in
      if gone { drafter.dropDraft() }
    }
  }

  private var working: some View {
    HStack(spacing: 10) {
      ProgressView().controlSize(.small)
      Text(drafter.draft == nil ? "Writing the test…" : "Changing the draft…")
      Spacer()
      Button("Stop") { drafter.conversation.stop() }
        .keyboardShortcut(.cancelAction)
    }
  }

  private var describing: some View {
    Group {
      Label("Describe the Test", systemImage: "sparkles")
        .font(.headline)
        .accessibilityAddTraits(.isHeader)
      TextField(
        text: $drafter.description,
        prompt: Text("Open Settings, turn on Dark Mode and check that the window is dark. If a sign-in window appears, close it first."),
        axis: .vertical
      ) {
        Text("What the test does")
      }
      .labelsHidden()
      .lineLimit(4...10)
      .focused($focused)
      .accessibilityLabel(Text("What the test does"))
      Text("The AI writes the blocks of this test from your words. You see them on the canvas before anything is saved.")
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      problem
      HStack {
        Spacer()
        Button("Cancel", role: .cancel) { close() }
          .keyboardShortcut(.cancelAction)
        Button("Write Test") { write() }
          .keyboardShortcut(.defaultAction)
          .disabled(drafter.description.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
      }
    }
  }

  @ViewBuilder private var problem: some View {
    switch drafter.blocked {
    case .noApp:
      Label("Choose the app the test runs in first, in the toolbar.", systemImage: "exclamationmark.triangle.fill")
        .foregroundStyle(.orange)
    case .noModel:
      HStack {
        Label("Choose an AI model in Settings → AI first.", systemImage: "exclamationmark.triangle.fill")
          .foregroundStyle(.orange)
        Spacer()
        Button("Open Settings") { openSettings() }
      }
    case nil:
      if case .failed(let message) = drafter.conversation.phase {
        Label(message, systemImage: "exclamationmark.triangle.fill")
          .foregroundStyle(.red)
          .textSelection(.enabled)
          .fixedSize(horizontal: false, vertical: true)
      }
    }
  }

  private func result(_ draft: TestDraft) -> some View {
    Group {
      Label("Draft", systemImage: "sparkles")
        .font(.headline)
        .accessibilityAddTraits(.isHeader)
      if !draft.summary.isEmpty {
        Text(verbatim: draft.summary)
          .fixedSize(horizontal: false, vertical: true)
      }
      Text("Check the blocks on the canvas. Nothing is saved until you accept the draft.")
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      let warnings = document.issues.filter { $0.severity == .warning }
      if !warnings.isEmpty {
        Label {
          Text("\(warnings.count) blocks need attention, such as reference images to take. They’re marked on the canvas.")
        } icon: {
          Image(systemName: "exclamationmark.triangle").foregroundStyle(.orange)
        }
        .font(.callout)
        .fixedSize(horizontal: false, vertical: true)
      }
      AIToolsUsed(calls: draft.calls, unavailable: draft.unavailable)
      TextField(text: $drafter.feedback, prompt: Text("Also check the total in the cart"), axis: .vertical) {
        Text("What to change")
      }
      .lineLimit(1...4)
      .focused($focused)
      .accessibilityLabel(Text("What to change"))
      problem
      HStack {
        Button("Cancel", role: .cancel) { close() }
          .keyboardShortcut(.cancelAction)
          .help(Text("Drop the draft and keep the test as it was"))
        Spacer()
        Button("Rework") { write(rework: true) }
          .disabled(drafter.feedback.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
          .help(Text("Ask the AI to change the draft as you wrote"))
        Button("Accept") { accept() }
          .keyboardShortcut(.defaultAction)
          .help(Text("Make the draft the test. Undo brings the old test back."))
      }
    }
  }

  private func write(rework: Bool = false) {
    drafter.write(
      document, localOnly: AISettings.app.localOnly || project.settings.localOnly, project: project.settings, rework: rework)
  }

  private func accept() {
    document.acceptDraft()
    document.drafter = nil
    AccessibilityNotification.Announcement(String(localized: "The draft is saved as the test.")).post()
  }

  private func close() {
    drafter.conversation.stop()
    document.discardDraft()
    document.drafter = nil
  }
}
