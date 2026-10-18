import Foundation
import JEVisionCore
import Observation

/// Git for the open project: where it stands and what can be done with it. Git runs off the
/// main thread; one action at a time.
@MainActor
@Observable
final class ProjectGit {
  enum State: Equatable {
    /// No project, or not looked at yet.
    case unknown
    case notInstalled
    /// The project folder isn't in a repository.
    case untracked
    case tracked(GitStatus)
  }

  enum Activity {
    case starting, committing, pulling, pushing, switching, proposing
  }

  /// Who the app works with GitHub as: GitHub CLI's sign-in.
  enum GitHubSignIn: Equatable {
    case checking
    case signedIn(GitHubAccount)
    /// Not signed in, or GitHub couldn't be asked, and why.
    case signedOut(GitError)
  }

  /// An action that didn't work, and why, in words for the user.
  struct Failure: Equatable {
    let activity: Activity
    let message: String
  }

  private(set) var state = State.unknown
  /// Branches here, then those only on the server.
  private(set) var branches: [GitBranch] = []
  private(set) var activity: Activity?
  private(set) var gitHub = GitHubSignIn.checking
  /// What went wrong with the last action, for an alert.
  var failure: Failure?
  /// The New Pull Request sheet is shown over the main window.
  var isComposingPullRequest = false

  @ObservationIgnored private(set) var project: URL?
  @ObservationIgnored private var refreshTask: Task<Void, Never>?

  var status: GitStatus? {
    if case .tracked(let status) = state { status } else { nil }
  }

  func open(_ url: URL?) {
    project = url
    state = .unknown
    branches = []
    refresh()
  }

  /// Looks at the repository again. Calls in quick succession are merged into one look.
  func refresh() {
    guard let project else { return }
    refreshTask?.cancel()
    refreshTask = Task { [weak self] in
      try? await Task.sleep(for: .milliseconds(150))
      guard !Task.isCancelled else { return }
      let path = project.path(percentEncoded: false)
      let found = await Task.detached(priority: .utility) { () -> (State, [GitBranch]) in
        do {
          guard let status = try gitStatus(project: path) else { return (.untracked, []) }
          return (.tracked(status), (try? gitBranches(project: path)) ?? [])
        } catch GitError.NotInstalled {
          return (.notInstalled, [])
        } catch {
          return (.untracked, [])
        }
      }.value
      guard let self, !Task.isCancelled, self.project == project else { return }
      if state != found.0 { state = found.0 }
      if branches != found.1 { branches = found.1 }
    }
  }

  func startTracking() {
    perform(.starting) { project in
      _ = try gitStartTracking(project: project)
    } then: { [weak self] _ in
      guard let url = self?.project, GitIgnore.needsEntry(in: url) else { return }
      try? GitIgnore.add(to: url)
    }
  }

  /// Commits the changes to `paths` with the user's message, first saving who they are when
  /// git doesn't know yet. Calls `done` once committed.
  func commit(_ paths: [String], message: String, as identity: GitIdentity?, done: @escaping () -> Void) {
    perform(.committing) { project in
      if let identity { try gitSetIdentity(project: project, identity: identity) }
      _ = try gitCommit(project: project, paths: paths, message: message)
    } then: { _ in
      done()
    }
  }

  func pull() {
    perform(.pulling) { try gitPull(project: $0) }
  }

  func push() {
    perform(.pushing) { try gitPush(project: $0) }
  }

  func switchTo(_ branch: String) {
    perform(.switching) { try gitSwitchBranch(project: $0, name: branch) }
  }

  /// Makes a branch and switches to it. Calls `done` once made.
  func createBranch(_ name: String, done: @escaping () -> Void) {
    perform(.switching) { try gitCreateBranch(project: $0, name: name) } then: { _ in done() }
  }

  /// Asks GitHub CLI again who is signed in.
  func checkGitHub() {
    Task {
      let result = await Task.detached(priority: .utility) { Result { try githubAccount() } }.value
      switch result {
      case .success(let account): gitHub = .signedIn(account)
      case .failure(let error as GitError): gitHub = .signedOut(error)
      case .failure(let error): gitHub = .signedOut(.Failed(message: error.localizedDescription))
      }
    }
  }

  /// Opens a pull request from the current branch for `base`, pushing the branch first when
  /// `push`. Calls `done` with it once open.
  func createPullRequest(
    base: String, title: String, description: String, push: Bool, done: @escaping (PullRequest) -> Void
  ) {
    perform(.proposing) { project in
      try githubCreatePullRequest(project: project, base: base, title: title, description: description, push: push)
    } then: { done($0) }
  }

  /// Who commits are signed with; empty fields when git doesn't know.
  func identity() async -> GitIdentity {
    let empty = GitIdentity(name: "", email: "")
    guard let path = project?.path(percentEncoded: false) else { return empty }
    return await Task.detached { (try? gitIdentity(project: path)) ?? empty }.value
  }

  private func perform<Value: Sendable>(
    _ activity: Activity, _ work: @escaping @Sendable (String) throws -> Value,
    then: @escaping (Value) -> Void = { _ in }
  ) {
    guard self.activity == nil, let project else { return }
    self.activity = activity
    let path = project.path(percentEncoded: false)
    Task {
      let result = await Task.detached(priority: .userInitiated) { Result { try work(path) } }.value
      self.activity = nil
      switch result {
      case .success(let value): then(value)
      case .failure(let error): failure = Failure(activity: activity, message: GitText.failure(error))
      }
      refresh()
    }
  }
}
