import Foundation
import JEVisionCore
import Observation

/// The runs CI made for the open project, fetched from GitHub into its history. Fetching runs off
/// the main thread; one at a time.
@MainActor
@Observable
final class ProjectCI {
  enum State: Equatable {
    /// Not asked yet, or no project.
    case idle
    case checking
    case done(CiRefresh)
    /// GitHub isn't signed in: a hint, not a failure.
    case needsSignIn
    /// The project isn't in a git repository that is on GitHub: there is nothing to ask.
    case notOnGitHub
    /// GitHub couldn't be asked, and why, in words for the user.
    case failed(String)
  }

  private(set) var state = State.idle

  /// Called on the main thread when runs were added to the history.
  @ObservationIgnored var imported: () -> Void = {}
  @ObservationIgnored private var project: URL?
  @ObservationIgnored private var task: Task<Void, Never>?

  func open(_ url: URL?) {
    task?.cancel()
    task = nil
    project = url
    state = .idle
  }

  /// Fetches the runs CI uploaded since last time. A fetch that is going on is not started again.
  func refresh() {
    guard let path = project?.path(percentEncoded: false), task == nil else { return }
    state = .checking
    task = Task {
      let result = await Task.detached(priority: .utility) { Result { try refreshCiRuns(project: path) } }.value
      guard !Task.isCancelled else { return }
      task = nil
      state = Self.state(after: result)
      if case .success(let found) = result, found.imported > 0 { imported() }
    }
  }

  /// What went wrong with a fetch, when something did and the user should know; a missing
  /// sign-in or a project that isn't on GitHub is not that.
  nonisolated static func problem(in state: State) -> String? {
    switch state {
    case .failed(let message): message
    case .done(let found) where found.failed > 0:
      String(localized: "Some CI results couldn’t be fetched: they are too large, broken or gone.")
    default: nil
    }
  }

  /// What a finished fetch says to the user.
  nonisolated static func state(after result: Result<CiRefresh, any Error>) -> State {
    switch result {
    case .success(let found):
      return .done(found)
    case .failure(let error):
      switch error as? GitError {
      case .GitHubSignedOut: return .needsSignIn
      case .NotInstalled, .NotRepository, .NoRemote, .NotGitHub: return .notOnGitHub
      default: return .failed(GitText.failure(error))
      }
    }
  }
}
