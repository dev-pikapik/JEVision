import JEVisionCore
import SwiftUI

/// Words for git's states and problems, for people who have never used git.
enum GitText {
  static func failure(_ error: any Error) -> String {
    guard let error = error as? GitError else { return error.localizedDescription }
    switch error {
    case .NotInstalled:
      return String(localized: "Git isn’t installed on this Mac.")
    case .NotRepository:
      return String(localized: "Changes to this project aren’t tracked.")
    case .NoCommits:
      return String(localized: "There’s nothing to send yet. Commit your changes first.")
    case .NoRemote:
      return String(localized: "This project isn’t connected to a server.")
    case .NoUpstream(let branch):
      return String(localized: "Branch “\(branch)” isn’t on the server yet. Push it first.")
    case .Detached:
      return String(localized: "No branch is selected. Choose a branch first.")
    case .SignIn:
      return String(localized: "The server didn’t accept your sign-in. Sign in with GitHub CLI (gh auth login) or set up an SSH key, then try again.")
    case .RemoteNotFound:
      return String(localized: "The repository wasn’t found on the server, or your account can’t open it.")
    case .Unreachable:
      return String(localized: "Can’t reach the server. Check the internet connection and try again.")
    case .Rejected:
      return String(localized: "The server has changes you don’t have yet. Pull first, then push again.")
    case .Conflict(let paths):
      return String(localized: "The same part of \(names(paths)) changed here and on the server, so the changes can’t be combined automatically. Nothing was changed; your files are as they were.")
    case .LocalChanges(let paths):
      return String(localized: "\(names(paths)) has changes that aren’t committed and would be replaced. Commit them first, then try again.")
    case .NoIdentity:
      return String(localized: "Enter your name and email: they’re saved with each commit.")
    case .EmptyMessage:
      return String(localized: "Describe what changed.")
    case .NothingToCommit:
      return String(localized: "There are no changes to commit.")
    case .BadBranchName(let name):
      return String(localized: "“\(name)” can’t be a branch name. Use letters, numbers and dashes, without spaces.")
    case .BranchExists(let name):
      return String(localized: "There’s already a branch named “\(name)”.")
    case .NotInCommit:
      return String(localized: "That version of the file isn’t there.")
    case .NoGitHubCli:
      return String(localized: "GitHub CLI isn’t installed. Install it from cli.github.com, then run “gh auth login” in Terminal.")
    case .GitHubSignedOut:
      return String(localized: "You’re not signed in to GitHub. Run “gh auth login” in Terminal, then try again.")
    case .NotGitHub:
      return String(localized: "This project’s server isn’t GitHub.")
    case .NoPermission:
      return String(localized: "Your GitHub account isn’t allowed to do this in this repository.")
    case .NotPushed(let branch):
      return String(localized: "Branch “\(branch)” has commits that aren’t on GitHub yet. Push it first.")
    case .PullRequestExists:
      return String(localized: "This branch already has an open pull request.")
    case .NothingToPropose(let base):
      return String(localized: "This branch has no commits that “\(base)” doesn’t have yet.")
    case .Failed(let message):
      return message
    }
  }

  /// “login” or “login and 2 more”.
  static func names(_ paths: [String]) -> String {
    guard let first = paths.first else { return "" }
    let name = BlockSummary.quoted(fileName(first), limit: 40)
    return paths.count == 1 ? name : String(localized: "\(name) and \(paths.count - 1) more")
  }

  /// A test's name without its extension; other files by their file name.
  static func fileName(_ path: String) -> String {
    let file = (path as NSString).lastPathComponent
    return ProjectScanner.isTestFile(file) ? ProjectScanner.testName(file) : file
  }

  static func isTest(_ path: String) -> Bool { ProjectScanner.isTestFile((path as NSString).lastPathComponent) }

  static func isPicture(_ path: String) -> Bool { path.lowercased().hasSuffix(".png") }

  static func symbol(_ kind: ChangeKind) -> String {
    switch kind {
    case .added: "plus.circle.fill"
    case .modified: "pencil.circle.fill"
    case .deleted: "minus.circle.fill"
    case .renamed: "arrow.right.circle.fill"
    case .conflicted: "exclamationmark.triangle.fill"
    }
  }

  static func color(_ kind: ChangeKind) -> Color {
    switch kind {
    case .added: .green
    case .modified: .orange
    case .deleted, .conflicted: .red
    case .renamed: .blue
    }
  }

  static func title(_ kind: ChangeKind) -> String {
    switch kind {
    case .added: String(localized: "Added")
    case .modified: String(localized: "Changed")
    case .deleted: String(localized: "Deleted")
    case .renamed: String(localized: "Renamed")
    case .conflicted: String(localized: "Conflict")
    }
  }

  /// “Changed: 2 tests and 1 other file”.
  static func summary(_ changes: [FileChange]) -> String {
    guard !changes.isEmpty else { return String(localized: "No changes since the last commit") }
    let tests = changes.filter { isTest($0.path) }.count
    let others = changes.count - tests
    var parts: [String] = []
    if tests > 0 { parts.append(String(localized: "\(tests) tests")) }
    if others > 0 {
      parts.append(tests > 0 ? String(localized: "\(others) other files") : String(localized: "\(others) files"))
    }
    return String(localized: "Changed: \(parts.formatted(.list(type: .and)))")
  }

  /// How the branch compares with the server.
  static func sync(_ status: GitStatus) -> String? {
    guard status.remote != nil, status.branch != nil else { return nil }
    guard status.upstream != nil else { return String(localized: "This branch isn’t on the server yet") }
    guard status.ahead > 0 || status.behind > 0 else { return String(localized: "Up to date with the server") }
    var parts: [String] = []
    if status.ahead > 0 { parts.append(String(localized: "\(Int(status.ahead)) commits to push")) }
    if status.behind > 0 { parts.append(String(localized: "\(Int(status.behind)) commits to pull")) }
    return parts.joined(separator: ", ")
  }
}
