import AppKit
import JEVisionCore
import JEVisionHands
import SwiftUI

/// Settings → Advanced → Command Line: whether `jevision` is installed, and how to run the project with it.
struct CommandLineSection: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.openWindow) private var openWindow
  @State private var state = CommandLineTool.State.notInstalled
  private let tool = CommandLineTool.bundled

  var body: some View {
    Section {
      if let reason = tool.installUnavailableReason {
        Text(reason)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      } else {
        LabeledContent {
          Button(isInstalled ? "Change…" : "Install…") { openWindow(id: SceneID.commandLineTool) }
        } label: {
          RowLabel(
            Text("Run tests from Terminal and scripts"),
            isInstalled ? Text("Installed. The same engine as the app, without a window.") : Text("The same engine as the app, without a window."))
        }
        .settingAnchor("commandLine")
        if let root = project.rootURL {
          CommandText("jevision run \(CommandLineTool.quoted(root.path)) --junit junit.xml --html reports")
        }
      }
    } header: {
      Text("Command Line")
    } footer: {
      if tool.installUnavailableReason == nil, project.rootURL != nil {
        Text("Runs every test of the open project and saves the results as JUnit XML and HTML reports.")
          .font(.callout)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
    }
    .onAppear(perform: refresh)
    // The install window is another window of this app; look again when Settings is in front again.
    .onReceive(NotificationCenter.default.publisher(for: NSWindow.didBecomeKeyNotification)) { _ in refresh() }
  }

  private var isInstalled: Bool {
    if case .installed = state { true } else { false }
  }

  private func refresh() {
    state = tool.state()
  }
}

/// Settings → Advanced → Continuous Integration: a GitHub Actions workflow for the open project.
struct ContinuousIntegrationSection: View {
  @Environment(ProjectStore.self) private var project
  @State private var workflow: CiWorkflowFile?
  @State private var copied = false
  @State private var adding = false

  var body: some View {
    Section {
      if project.rootURL != nil {
        ActionRow {
          Button(copied ? "Copied" : "Copy Workflow", systemImage: copied ? "checkmark" : "doc.on.doc") {
            guard let workflow else { return }
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(workflow.text, forType: .string)
            copied = true
          }
          Button("Add to Project…") { adding = true }
            .disabled(workflow?.path == nil)
        } label: {
          RowLabel(Text("Run tests on every pull request"), Text("GitHub Actions, on a Mac of your own"))
        }
        .settingAnchor("continuousIntegration")
        if let workflow, workflow.path == nil {
          Text("The project folder isn’t inside a git repository, so there is nowhere to add the workflow. Copy it to use it elsewhere.")
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        }
      } else {
        Text("Open a project to get a workflow for it.")
          .foregroundStyle(.secondary)
      }
    } header: {
      Text("Continuous Integration")
    } footer: {
      if project.rootURL != nil {
        Text("GitHub’s own Mac runners have no logged-in user, so the tests run on a self-hosted runner: a Mac of yours that stays logged in. Add the secrets your tests use to the repository’s secrets under the same names.")
          .font(.callout)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
    }
    .task(id: project.items) { await load() }
    .sheet(isPresented: $adding) {
      if let workflow, let root = project.rootURL {
        CIWorkflowSheet(workflow: workflow, project: root, tests: Self.testFiles(in: project.items)) {
          Task { await load() }
        }
      }
    }
  }

  private func load() async {
    copied = false
    guard let root = project.rootURL?.path(percentEncoded: false) else {
      workflow = nil
      return
    }
    let tests = Self.testFiles(in: project.items)
    workflow = await Task.detached(priority: .utility) { ciWorkflow(project: root, tests: tests) }.value
  }

  /// Every test file of the project.
  static func testFiles(in items: [ProjectItem]) -> [String] {
    items.flatMap { $0.kind == .test ? [$0.url.path(percentEncoded: false)] : testFiles(in: $0.children) }
  }
}

/// Shows the workflow file before it is written into the repository, and asks before it replaces
/// one that is already there.
private struct CIWorkflowSheet: View {
  let workflow: CiWorkflowFile
  let project: URL
  let tests: [String]
  let added: () -> Void
  @Environment(\.dismiss) private var dismiss
  @State private var written: String?
  @State private var failure: String?

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      Text(written == nil ? "Add the Workflow to the Project" : "Workflow Added")
        .font(.title2.bold())
        .accessibilityAddTraits(.isHeader)
      if let written {
        Label("The file is in your repository. Commit and push it, and GitHub runs the tests from then on.", systemImage: "checkmark.circle.fill")
          .foregroundStyle(.green)
          .fixedSize(horizontal: false, vertical: true)
        Text(verbatim: written)
          .font(.callout.monospaced())
          .textSelection(.enabled)
          .foregroundStyle(.secondary)
      } else {
        Text("JEVision creates this file in your repository. Nothing is committed or pushed.")
          .fixedSize(horizontal: false, vertical: true)
        if let path = workflow.path {
          Text(verbatim: path)
            .font(.callout.monospaced())
            .textSelection(.enabled)
            .foregroundStyle(.secondary)
        }
        if workflow.exists {
          Label("This file is already there. Adding the workflow replaces it.", systemImage: "exclamationmark.triangle.fill")
            .foregroundStyle(.orange)
            .fixedSize(horizontal: false, vertical: true)
        }
        ScrollView {
          Text(verbatim: workflow.text)
            .font(.caption.monospaced())
            .textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(10)
        }
        .frame(height: 280)
        .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
      }
      Spacer(minLength: 0)
      HStack {
        if let written {
          Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([URL(filePath: written)]) }
          Spacer()
          Button("Done") { dismiss() }
            .keyboardShortcut(.defaultAction)
        } else {
          Spacer()
          Button("Cancel", role: .cancel) { dismiss() }
          Button(workflow.exists ? "Replace" : "Add", role: workflow.exists ? .destructive : nil, action: write)
            .keyboardShortcut(.defaultAction)
        }
      }
    }
    .padding(20)
    .frame(width: 560, height: 520)
    .alert(
      "Couldn’t add the workflow",
      isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } }),
      presenting: failure
    ) { _ in
      Button("OK") {}
    } message: { Text($0) }
  }

  private func write() {
    do {
      written = try addCiWorkflow(project: project.path(percentEncoded: false), tests: tests, replace: workflow.exists)
      AppLog.info("The CI workflow was added to the project", "project")
      added()
    } catch {
      AppLog.error("The CI workflow couldn’t be added: \(error)", "project")
      failure = Self.message(error)
    }
  }

  static func message(_ error: any Error) -> String {
    switch error {
    case CiError.NotInRepository:
      String(localized: "The project folder isn’t inside a git repository.")
    case CiError.Exists:
      String(localized: "The repository already has this workflow.")
    case CiError.Unwritable(let reason):
      String(localized: "The file can’t be written: \(reason)")
    default:
      error.localizedDescription
    }
  }
}

/// Settings → Advanced → Reset: puts every setting back and opens the app again.
struct ResetSettingsSection: View {
  @State private var confirms = false
  @State private var restartFailed = false

  var body: some View {
    Section {
      LabeledContent {
        Button("Reset…", role: .destructive) { confirms = true }
      } label: {
        RowLabel(Text("Reset all settings"), Text("Your tests, the open project, secrets and API keys stay."))
      }
      .settingAnchor("resetAll")
    }
    .confirmationDialog("Reset all settings?", isPresented: $confirms, titleVisibility: .visible) {
      Button("Reset and Restart", role: .destructive, action: reset)
    } message: {
      Text("Every setting goes back to how it was when JEVision was first installed, and JEVision opens again.")
    }
    .alert("Couldn’t restart JEVision", isPresented: $restartFailed) {
      Button("OK") {}
    } message: {
      Text("Settings are reset. Quit JEVision and open it again to finish.")
    }
  }

  private func reset() {
    SettingsReset.reset(Profile.current.defaults, domain: Profile.current.defaultsDomain)
    restartFailed = !Relaunch.now()
  }
}

@MainActor
enum SettingsReset {
  /// Not settings: the open and the recent projects, where “My Tests” is, and whether the
  /// welcome window was seen.
  static let kept: Set<String> = [
    ProjectStore.Key.recent, ProjectStore.Key.current, ProjectStore.Key.selection, ProjectStore.Key.myTests,
    OnboardingView.doneKey,
  ]

  static func reset(_ defaults: UserDefaults, domain: String) {
    for key in (defaults.persistentDomain(forName: domain) ?? [:]).keys where !kept.contains(key) {
      defaults.removeObject(forKey: key)
    }
  }
}
