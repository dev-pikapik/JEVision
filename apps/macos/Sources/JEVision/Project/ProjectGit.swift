import AppKit
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

  /// The Client ID of JEVision's OAuth app on GitHub, used when Settings names none.
  static let gitHubClientID = ""
  static let gitHubClientIDKey = "gitHubClientID"

  /// Who the app works with GitHub as.
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
  /// GitHub CLI's account, offered while signed out.
  private(set) var gitHubCLIAccount: GitHubAccount?
  /// The code to enter on GitHub while signing in in the browser.
  private(set) var signInCode: DeviceCode?
  /// What went wrong with the last action, for an alert.
  var failure: Failure?
  /// The New Pull Request sheet is shown over the main window.
  var isComposingPullRequest = false
  /// Files the last pull couldn't combine on its own, for the user to settle; nothing changed
  /// on disk until they do.
  var conflicts: [String]?

  @ObservationIgnored private(set) var project: URL?
  @ObservationIgnored private var refreshTask: Task<Void, Never>?
  @ObservationIgnored private var signInTask: Task<Void, Never>?

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

  /// Brings in the server's commits with the conflicted files as the user settled them and
  /// commits the merge. Calls `done` once committed.
  func finishPull(_ files: [SettledFile], done: @escaping () -> Void) {
    perform(.pulling) { try gitFinishPull(project: $0, files: files) } then: { _ in done() }
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

  /// Asks GitHub again who is signed in, and whether GitHub CLI has an account to offer.
  func checkGitHub() {
    Task { await show(Task.detached(priority: .utility) { Result { try githubAccount() } }.value) }
  }

  /// Signs in to GitHub in the browser: copies the code, opens GitHub's page for it and waits
  /// until the user has entered it there.
  func signInToGitHub(clientID: String) {
    cancelSignIn()
    signInTask = Task {
      do {
        let code = try await Task.detached { try githubSignInStart(clientId: clientID) }.value
        signInCode = code
        copySignInCode()
        openSignInPage()
        let account = try await waitForSignIn(code, clientID: clientID)
        signInCode = nil
        await show(.success(account))
      } catch is CancellationError {
      } catch {
        signInCode = nil
        await show(.failure(error))
      }
    }
  }

  /// Asks GitHub every `interval` seconds whether the code has been entered, until it expires.
  private func waitForSignIn(_ code: DeviceCode, clientID: String) async throws -> GitHubAccount {
    var interval = code.interval
    let expiry = ContinuousClock.now + .seconds(code.expiresIn)
    while ContinuousClock.now < expiry {
      try await Task.sleep(for: .seconds(interval))
      let step = await Task.detached { [interval] in
        Result { try githubSignInPoll(clientId: clientID, deviceCode: code.deviceCode, interval: interval) }
      }.value
      switch step {
      case .success(.waiting), .failure(GitError.Unreachable): continue
      case .success(.slowDown(let next)): interval = next
      case .success(.signedIn(let account)): return account
      case .failure(let error): throw error
      }
    }
    throw GitError.SignInExpired
  }

  func cancelSignIn() {
    signInTask?.cancel()
    signInTask = nil
    signInCode = nil
  }

  func copySignInCode() {
    guard let code = signInCode?.userCode else { return }
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString(code, forType: .string)
  }

  func openSignInPage() {
    guard let url = signInCode.flatMap({ URL(string: $0.verificationUri) }) else { return }
    NSWorkspace.shared.open(url)
  }

  /// Signs in with the account GitHub CLI is signed in to.
  func useGitHubCLIAccount() {
    Task { await show(Task.detached(priority: .utility) { Result { try githubUseCliAccount() } }.value) }
  }

  func signOutOfGitHub() {
    cancelSignIn()
    Task {
      let result = await Task.detached(priority: .utility) { Result { try githubSignOut() } }.value
      if case .failure(let error) = result {
        await show(.failure(error))
      } else {
        await show(.failure(GitError.GitHubSignedOut))
      }
    }
  }

  /// Shows who is signed in, or why no one is, then what GitHub CLI has to offer.
  private func show(_ result: Result<GitHubAccount, any Error>) async {
    switch result {
    case .success(let account):
      gitHub = .signedIn(account)
      gitHubCLIAccount = nil
    case .failure(let error):
      gitHub = .signedOut(error as? GitError ?? .Failed(message: error.localizedDescription))
      gitHubCLIAccount = await Task.detached(priority: .utility) { try? githubCliAccount() }.value
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

  /// The name notes are signed with: the one git signs commits with, else the one of the macOS account.
  func authorName() async -> String {
    let name = await identity().name
    return name.isEmpty ? NSFullUserName() : name
  }

  /// The people who committed to the project, the one git signs with first; none without git.
  func authors() async -> [String] {
    guard let path = project?.path(percentEncoded: false) else { return [] }
    return await Task.detached { (try? gitAuthors(project: path)) ?? [] }.value
  }

  /// What Git LFS can do about the file at `file`, a path in the project.
  enum LFS {
    /// It takes the file already.
    case tracked
    /// It is installed and can be told to take the file.
    case available
    /// The project isn't in git, or Git LFS isn't installed.
    case missing
  }

  func lfs(for file: String) async -> LFS {
    guard let path = project?.path(percentEncoded: false) else { return .missing }
    return await Task.detached {
      guard gitLfsAvailable(project: path) else { return .missing }
      return ((try? gitLfsTracks(project: path, path: file)) ?? false) ? .tracked : .available
    }.value
  }

  /// Makes Git LFS take the file at `file`, a path in the project; this writes `.gitattributes`.
  func trackWithLFS(_ file: String) async throws {
    guard let path = project?.path(percentEncoded: false) else { return }
    try await Task.detached { try gitLfsTrack(project: path, path: file) }.value
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
      case .failure(GitError.Conflict(let paths)) where activity == .pulling: conflicts = paths
      case .failure(let error): failure = Failure(activity: activity, message: GitText.failure(error))
      }
      refresh()
    }
  }
}
