import JEVisionCore
import SwiftUI

struct GitSettingsView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.openWindow) private var openWindow
  @State private var isNamingBranch = false
  @State private var branchName = ""
  @State private var isComposing = false

  private var git: ProjectGit { project.git }

  var body: some View {
    Form {
      Section {
        PageHeader(
          symbol: "arrow.triangle.branch", color: .orange, title: Text("Git and GitHub"),
          message: Text("History, commits, branches and pull requests — no terminal needed."))
      }
      if project.rootURL == nil {
        Section {
          LabeledContent {
            Button("Open Project…", action: project.choose)
          } label: {
            Text("No project is open")
            Text("Open the folder with your tests to see its changes.")
          }
        }
      } else {
        switch git.state {
        case .unknown:
          Section {
            ProgressView()
              .frame(maxWidth: .infinity)
              .accessibilityLabel(Text("Looking at the project"))
          }
        case .notInstalled:
          Section {
            LabeledContent {
              Button("Install…", action: installTools)
            } label: {
              Text("Git isn’t installed")
              Text("macOS installs it with the Command Line Developer Tools, in a few minutes.")
            }
          }
        case .untracked:
          Section {
            LabeledContent {
              Button("Start Tracking Changes", action: git.startTracking)
                .disabled(git.activity != nil)
            } label: {
              Text("Changes to “\(project.name)” aren’t tracked")
              Text("Git keeps every version of your tests, so you can see what changed and go back to any version.")
            }
          }
        case .tracked(let status):
          tracked(status)
        }
      }
      gitHubSection
    }
    .formStyle(.grouped)
    .onAppear {
      git.refresh()
      git.checkGitHub()
    }
    .sheet(isPresented: $isComposing) { NewPullRequestView() }
    .alert("New Branch", isPresented: $isNamingBranch) {
      TextField("Name", text: $branchName, prompt: Text(verbatim: "new-login-screen"))
      Button("Create") { git.createBranch(branchName.trimmingCharacters(in: .whitespaces)) { branchName = "" } }
      Button("Cancel", role: .cancel) {}
    } message: {
      Text("A branch keeps your changes apart until they’re ready. Your current changes come along.")
    }
    .alert(
      failureTitle, isPresented: Binding(
        get: { git.failure.map { ![.committing, .proposing].contains($0.activity) } ?? false }, set: { if !$0 { git.failure = nil } }),
      presenting: git.failure
    ) { _ in
      Button("OK") {}
    } message: { Text($0.message) }
  }

  @ViewBuilder
  private func tracked(_ status: GitStatus) -> some View {
    Section {
      LabeledContent {
        if let remote = status.remote {
          Text(verbatim: remote)
            .textSelection(.enabled)
        } else {
          Text("Only on this Mac")
        }
      } label: {
        Text("Repository")
        if let sync = GitText.sync(status) { Text(sync) }
      }
      LabeledContent("Branch") {
        Menu(status.branch ?? String(localized: "No branch")) {
          let local = git.branches.filter { !$0.isRemote }
          let remote = git.branches.filter(\.isRemote)
          ForEach(local, id: \.name) { branch in
            Button { git.switchTo(branch.name) } label: {
              if branch.isCurrent { Label(branch.name, systemImage: "checkmark") } else { Text(verbatim: branch.name) }
            }
          }
          if !remote.isEmpty {
            Section("On the Server") {
              ForEach(remote, id: \.name) { branch in
                Button(branch.name) { git.switchTo(branch.name) }
              }
            }
          }
          Divider()
          Button("New Branch…") { isNamingBranch = true }
            .disabled(!status.hasCommits)
        }
        .fixedSize()
        .disabled(git.activity != nil)
      }
      LabeledContent {
        HStack {
          if let activity = git.activity {
            ProgressView().controlSize(.small)
              .accessibilityLabel(Text(activityText(activity)))
          }
          Button("Commit…") { openWindow(id: SceneID.commit) }
            .disabled(status.changes.isEmpty)
          if status.upstream != nil {
            Button("Pull", action: git.pull)
              .help(Text("Get the latest commits from the server"))
              .disabled(git.activity != nil)
          }
          if status.remote != nil {
            Button("Push", action: git.push)
              .help(Text("Send your commits to the server"))
              .disabled(git.activity != nil || !status.hasCommits)
          }
        }
      } label: {
        Text("Changes")
        Text(GitText.summary(status.changes))
      }
    } header: {
      Text("This Project")
    } footer: {
      VStack(alignment: .leading, spacing: 6) {
        Text("Commits contain only what you write. JEVision adds nothing to them.")
        if status.remote != nil {
          Text("Signing in to the server uses your Mac’s git setup: GitHub CLI, passwords in the Keychain or SSH keys.")
        }
      }
      .font(.callout)
      .foregroundStyle(.secondary)
      .fixedSize(horizontal: false, vertical: true)
    }
  }

  @ViewBuilder
  private var gitHubSection: some View {
    Section {
      LabeledContent {
        switch git.gitHub {
        case .checking:
          ProgressView().controlSize(.small)
            .accessibilityLabel(Text("Checking the GitHub account"))
        case .signedIn(let account):
          Text(verbatim: account.login)
            .textSelection(.enabled)
        case .signedOut(let problem):
          HStack {
            if problem == .NoGitHubCli || problem == .GitHubSignedOut {
              Button("Connect GitHub…") { GitHubConnect.start() }
                .help(Text("Opens Terminal to sign in to GitHub in your browser"))
            }
            Button("Check Again", action: git.checkGitHub)
          }
        }
      } label: {
        Text("Account")
        switch git.gitHub {
        case .checking: EmptyView()
        case .signedIn(let account):
          if !account.name.isEmpty { Text(verbatim: account.name) }
        case .signedOut(let problem): Text(signInHelp(problem))
        }
      }
      if case .tracked(let status) = git.state, status.github != nil {
        LabeledContent {
          HStack {
            Button("New…") { isComposing = true }
              .help(Text("Propose the changes on your branch"))
              .disabled(git.activity != nil || !status.hasCommits || status.branch == nil)
            Button("Show…") { openWindow(id: SceneID.pullRequests) }
          }
        } label: {
          Text("Pull Requests")
          Text("Propose your branch’s changes, or see what others propose.")
        }
      }
    } header: {
      Text("GitHub")
    } footer: {
      Text("JEVision uses the account GitHub CLI is signed in to. To use another one, run “gh auth login” in Terminal.")
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
    }
  }

  private func signInHelp(_ problem: GitError) -> String {
    switch problem {
    case .NoGitHubCli, .GitHubSignedOut:
      String(localized: "Connect GitHub to sign in once in your browser, then check again.")
    default: GitText.failure(problem)
    }
  }

  private var failureTitle: Text {
    switch git.failure?.activity {
    case .pulling: Text("Couldn’t pull")
    case .pushing: Text("Couldn’t push")
    case .switching: Text("Couldn’t switch branches")
    case .starting: Text("Couldn’t start tracking changes")
    default: Text("Something went wrong")
    }
  }

  private func activityText(_ activity: ProjectGit.Activity) -> String {
    switch activity {
    case .starting: String(localized: "Starting to track changes…")
    case .committing: String(localized: "Committing…")
    case .pulling: String(localized: "Pulling…")
    case .pushing: String(localized: "Pushing…")
    case .switching: String(localized: "Switching branches…")
    case .proposing: String(localized: "Creating the pull request…")
    }
  }

  /// Asks macOS to install the Command Line Developer Tools, which include git.
  private func installTools() {
    let process = Process()
    process.executableURL = URL(filePath: "/usr/bin/xcode-select")
    process.arguments = ["--install"]
    try? process.run()
  }
}
