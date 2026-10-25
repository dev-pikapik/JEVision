import AppKit
import JEVisionCore
import JEVisionHands
import SwiftUI

/// A bug report about a failed run, filed as an issue of the project's GitHub repository: the
/// drafted words to read and change, the screenshots to include, and where it goes. Nothing is
/// sent until the user sends it.
struct BugReportSheet: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  @Environment(\.openURL) private var openURL
  @Environment(\.openSettings) private var openSettings
  /// The run's folder.
  let dir: String

  @State private var draft: BugReport?
  @State private var loadFailure: String?
  @State private var title = ""
  @State private var text = ""
  @State private var included: Set<String> = []
  @State private var github = GitHubState.checking
  @State private var addsToOpenIssue = true
  @State private var isSending = false
  @State private var sendFailure: String?
  @State private var sent: URL?

  enum GitHubState {
    case checking
    case ready(open: GitHubIssue?)
    case unavailable(GitError)
  }

  var body: some View {
    VStack(spacing: 0) {
      if let sent {
        done(sent)
      } else if let draft {
        form(draft)
      } else if let loadFailure {
        ContentUnavailableView {
          Label("Can’t write the bug report", systemImage: "exclamationmark.triangle")
        } description: {
          Text(loadFailure)
        }
        .frame(maxHeight: .infinity)
        footer { Button("Close", role: .cancel) { dismiss() } }
      } else {
        ProgressView()
          .frame(maxWidth: .infinity, maxHeight: .infinity)
          .accessibilityLabel(Text("Writing the bug report"))
      }
    }
    .frame(width: 640, height: 700)
    .task { await load() }
    .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
      // Back from the browser or Terminal after signing in to GitHub.
      if case .unavailable = github, let draft { Task { await check(draft) } }
    }
    .onChange(of: project.git.gitHub) {
      if case .unavailable = github, let draft { Task { await check(draft) } }
    }
    .alert(
      "Couldn’t send the bug report",
      isPresented: Binding(get: { sendFailure != nil }, set: { if !$0 { sendFailure = nil } }),
      presenting: sendFailure
    ) { _ in
      Button("OK") {}
    } message: { Text($0) }
  }

  @ViewBuilder
  private func form(_ draft: BugReport) -> some View {
    Form {
      Section {
        TextField("Title", text: $title)
        TextEditor(text: $text)
          .font(.body.monospaced())
          .frame(minHeight: 230, maxHeight: 300)
          .accessibilityLabel(Text("Description"))
      } header: {
        Text("Bug Report")
      } footer: {
        Text("Change anything you like. Secret values are replaced with •••• before sending.")
          .foregroundStyle(.secondary)
      }
      if !draft.screenshots.isEmpty {
        Section {
          ScrollView(.horizontal) {
            HStack(spacing: 12) {
              ForEach(draft.screenshots, id: \.label) { shot in
                screenshot(shot)
              }
            }
            .padding(.vertical, 4)
          }
        } header: {
          Text("Screenshots of the Failed Step")
        } footer: {
          Text("Areas the project always hides are painted over. Screenshots are saved in the repository’s “jevision-attachments” branch, so people who can open the repository can see them.")
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        }
      }
      DiagnosticsSection(text: $text)
      destination
    }
    .formStyle(.grouped)
    .disabled(isSending)
    footer {
      if isSending {
        ProgressView().controlSize(.small)
          .accessibilityLabel(Text("Sending…"))
      }
      Button("Copy Text") { copy() }
        .help(Text("Copy the title and description, to file the bug somewhere else"))
      Spacer()
      Button("Cancel", role: .cancel) { dismiss() }
      Button(commentsOn == nil ? "Open Issue" : "Add Comment") { send(draft) }
        .keyboardShortcut(.defaultAction)
        .disabled(!canSend)
    }
  }

  private func screenshot(_ shot: BugScreenshot) -> some View {
    let isOn = Binding(
      get: { included.contains(shot.label) },
      set: { if $0 { included.insert(shot.label) } else { included.remove(shot.label) } })
    return VStack(spacing: 6) {
      Group {
        if let image = NSImage(contentsOfFile: shot.path) {
          Image(nsImage: image).resizable().scaledToFit()
        } else {
          Image(systemName: "photo").foregroundStyle(.secondary)
        }
      }
      .frame(width: 170, height: 110)
      .opacity(isOn.wrappedValue ? 1 : 0.4)
      .accessibilityHidden(true)
      Toggle(screenshotTitle(shot.label), isOn: isOn)
        .toggleStyle(.checkbox)
    }
  }

  private func screenshotTitle(_ label: String) -> String {
    switch label {
    case "Expected": String(localized: "Expected")
    case "Actual": String(localized: "Actual")
    case "Difference": String(localized: "Difference")
    default: label
    }
  }

  @ViewBuilder private var destination: some View {
    Section {
      switch github {
      case .checking:
        LabeledContent("GitHub") {
          ProgressView().controlSize(.small)
            .accessibilityLabel(Text("Checking GitHub"))
        }
      case .ready(let open?):
        Picker("Send to", selection: $addsToOpenIssue) {
          Text("Issue #\(open.number) “\(open.title)”, as a comment").tag(true)
          Text("A new issue").tag(false)
        }
        .pickerStyle(.radioGroup)
        Text("This failure is already reported in an open issue. Adding this run to it keeps the reports in one place.")
          .font(.callout)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      case .ready(nil):
        LabeledContent("Send to") {
          Text(verbatim: project.git.status?.github ?? "GitHub")
        }
      case .unavailable(let problem):
        unavailable(problem)
      }
    } header: {
      Text("Where It Goes")
    }
  }

  private func openGitSettings() {
    AppPreferences.set(SettingsTab.git.rawValue, forKey: SettingsTab.key, in: Profile.current.defaults)
    openSettings()
  }

  @ViewBuilder private func unavailable(_ problem: GitError) -> some View {
    switch problem {
    case .NoGitHubCli, .GitHubSignedOut, .SignIn:
      LabeledContent {
        Button("Open Git Settings…", action: openGitSettings)
      } label: {
        Text("GitHub isn’t connected")
        Text("Sign in once in your browser. JEVision checks again when you come back.")
      }
    case .NotRepository, .NoCommits, .NoRemote, .NotGitHub:
      LabeledContent {
        Button("Open Git Settings…", action: openGitSettings)
      } label: {
        Text("This project isn’t on GitHub")
        Text("Bug reports go to the project’s GitHub repository. Put the project on GitHub, or copy the text to file it elsewhere.")
      }
    default:
      LabeledContent {
        Button("Try Again") { if let draft { Task { await check(draft) } } }
      } label: {
        Text("Can’t reach GitHub")
        Text(GitText.failure(problem))
      }
    }
  }

  private func done(_ url: URL) -> some View {
    Group {
      ContentUnavailableView {
        Label(commentsOn == nil ? "The issue is open" : "The comment is added", systemImage: "checkmark.circle")
      } description: {
        Text(verbatim: url.absoluteString)
          .textSelection(.enabled)
      }
      .frame(maxHeight: .infinity)
      footer {
        Spacer()
        Button("Open on GitHub") { openURL(url) }
        Button("Done") { dismiss() }
          .keyboardShortcut(.defaultAction)
      }
    }
  }

  private func footer(@ViewBuilder _ buttons: () -> some View) -> some View {
    HStack { buttons() }
      .padding()
  }

  /// The open issue the report goes to as a comment.
  private var commentsOn: GitHubIssue? {
    if case .ready(let open?) = github, addsToOpenIssue { return open }
    return nil
  }

  private var canSend: Bool {
    guard case .ready = github, !isSending else { return false }
    return !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
  }

  private func copy() {
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString("# \(title)\n\n\(text)", forType: .string)
  }

  private func load() async {
    guard let root = project.rootURL?.path(percentEncoded: false) else { return }
    let dir = dir
    let result = await Task.detached(priority: .userInitiated) {
      Result { try draftBugReport(project: root, dir: dir, secrets: StoredSecrets(project: root)) }
    }.value
    switch result {
    case .success(let found?):
      draft = found
      title = found.title
      text = found.body
      included = Set(found.screenshots.map(\.label))
      await check(found)
    case .success(nil):
      loadFailure = String(localized: "This run didn’t fail, so there’s no bug to report.")
    case .failure(let error):
      loadFailure = RunHistory.reason(error)
    }
  }

  /// Whether GitHub is connected, and the open issue about the same failure.
  private func check(_ draft: BugReport) async {
    guard let root = project.rootURL?.path(percentEncoded: false) else { return }
    github = .checking
    let fingerprint = draft.fingerprint
    let result = await Task.detached(priority: .userInitiated) {
      Result { try githubOpenBugReport(project: root, fingerprint: fingerprint) }
    }.value
    switch result {
    case .success(let open): github = .ready(open: open)
    case .failure(let error as GitError): github = .unavailable(error)
    case .failure(let error): github = .unavailable(.Failed(message: error.localizedDescription))
    }
  }

  private func send(_ draft: BugReport) {
    guard let root = project.rootURL?.path(percentEncoded: false) else { return }
    let report = BugReport(
      title: title, body: text, fingerprint: draft.fingerprint,
      screenshots: draft.screenshots.filter { included.contains($0.label) })
    let dir = dir
    let comment = commentsOn?.number
    isSending = true
    Task {
      let result = await Task.detached(priority: .userInitiated) {
        Result {
          try githubSendBugReport(project: root, dir: dir, report: report, commentOn: comment, secrets: StoredSecrets(project: root))
        }
      }.value
      isSending = false
      switch result {
      case .success(let url): sent = URL(string: url)
      case .failure(let error): sendFailure = GitText.failure(error)
      }
    }
  }
}
