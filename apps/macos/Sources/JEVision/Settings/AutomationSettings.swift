import AppKit
import JEVisionCore
import SwiftUI

/// Settings → Advanced → Command Line: whether `jevision` is installed, and how to run the project with it.
struct CommandLineSection: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.openWindow) private var openWindow
  @State private var state = CommandLineTool.State.notInstalled
  private let tool = CommandLineTool.bundled

  var body: some View {
    Section {
      if let reason = tool.unavailableReason {
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
      if tool.unavailableReason == nil, project.rootURL != nil {
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
  @State private var secrets: [String] = []
  @State private var copied = false

  var body: some View {
    Section {
      if let root = project.rootURL {
        let workflow = CIWorkflow.text(testsPath: CIWorkflow.testsPath(of: root), secrets: secrets)
        LabeledContent {
          Button(copied ? "Copied" : "Copy Workflow", systemImage: copied ? "checkmark" : "doc.on.doc") {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(workflow, forType: .string)
            copied = true
          }
        } label: {
          RowLabel(Text("Run tests on every pull request"), Text("GitHub Actions, on a Mac of your own"))
        }
        .settingAnchor("continuousIntegration")
        Text(verbatim: workflow)
          .font(.caption.monospaced())
          .textSelection(.enabled)
          .fixedSize(horizontal: false, vertical: true)
          .frame(maxWidth: .infinity, alignment: .leading)
      } else {
        Text("Open a project to get a workflow for it.")
          .foregroundStyle(.secondary)
      }
    } header: {
      Text("Continuous Integration")
    } footer: {
      if project.rootURL != nil {
        Text("Save it in the repository as .github/workflows/ui-tests.yml. GitHub’s own Mac runners have no logged-in user, so the tests run on a self-hosted runner: a Mac of yours that stays logged in. Add the secrets your tests use to the repository’s secrets under the same names.")
          .font(.callout)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
    }
    .task(id: project.items) {
      copied = false
      let tests = CIWorkflow.testFiles(in: project.items)
      secrets = await Task.detached(priority: .utility) { CIWorkflow.secrets(of: tests) }.value
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
    SettingsReset.reset(.standard, domain: Bundle.main.bundleIdentifier ?? "")
    restartFailed = !Relaunch.now()
  }
}

/// A GitHub Actions workflow that runs a project's tests on a self-hosted Mac runner.
enum CIWorkflow {
  /// The project folder relative to the root of its git repository: where CI finds it after checkout.
  static func testsPath(of project: URL) -> String {
    let project = project.standardizedFileURL.resolvingSymlinksInPath()
    var folder = project
    while true {
      if FileManager.default.fileExists(atPath: folder.appending(path: ".git").path) {
        let rest = project.pathComponents.dropFirst(folder.pathComponents.count)
        return rest.isEmpty ? "." : rest.joined(separator: "/")
      }
      if folder.pathComponents.count <= 1 { return "." }
      folder = folder.deletingLastPathComponent()
    }
  }

  /// Every test file of the project.
  static func testFiles(in items: [ProjectItem]) -> [URL] {
    items.flatMap { $0.kind == .test ? [$0.url] : testFiles(in: $0.children) }
  }

  /// The secrets the tests read, each once, sorted. Files that can't be read are skipped.
  static func secrets(of tests: [URL]) -> [String] {
    var names = Set<String>()
    for url in tests {
      guard let text = try? String(contentsOf: url, encoding: .utf8), let flow = try? loadFlow(json: text) else { continue }
      names.formUnion(requiredSecrets(flow: flow))
    }
    return names.sorted()
  }

  static func text(testsPath: String, secrets: [String]) -> String {
    var lines = [
      "name: UI tests",
      "on:",
      "  pull_request:",
      "  workflow_dispatch:",
      "jobs:",
      "  ui-tests:",
      "    runs-on: [self-hosted, macOS]",
      "    steps:",
      "      - uses: actions/checkout@v4",
      "      - name: Run UI tests",
    ]
    if !secrets.isEmpty {
      lines.append("        env:")
      lines += secrets.map { "          \($0): ${{ secrets.\($0) }}" }
    }
    // A block scalar keeps any folder name out of YAML's way; the shell gets it quoted.
    let path = testsPath.allSatisfy { $0.isLetter || $0.isNumber || "._-/".contains($0) } ? testsPath : CommandLineTool.quoted(testsPath)
    lines += [
      "        run: |",
      "          jevision run \(path) --junit junit.xml --html reports",
      "      - uses: actions/upload-artifact@v4",
      "        if: always()",
      "        with:",
      "          name: ui-test-reports",
      "          path: |",
      "            junit.xml",
      "            reports/",
    ]
    return lines.joined(separator: "\n") + "\n"
  }
}

@MainActor
enum SettingsReset {
  /// Not settings: the open project and whether the welcome window was seen.
  static let kept: Set<String> = [
    ProjectStore.Key.bookmark, ProjectStore.Key.name, ProjectStore.Key.selection, OnboardingView.doneKey,
  ]

  static func reset(_ defaults: UserDefaults, domain: String) {
    for key in (defaults.persistentDomain(forName: domain) ?? [:]).keys where !kept.contains(key) {
      defaults.removeObject(forKey: key)
    }
  }
}
