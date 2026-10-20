import Foundation
import JEVisionCore
import JEVisionHands

/// What `jevision issue` was asked to do.
struct IssueRequest: Equatable {
  /// A test file, whose newest failed run is reported, or the folder of a run; `nil` reports the
  /// newest failed run of the project.
  var target: String?
  var project: String?
  var send = false
  var title: String?
  var commentOn: UInt64?
  var newIssue = false
}

/// `jevision issue`: drafts a bug report about a failed run and, with `--send`, files it in the
/// project's GitHub repository, as the app's Report Bug… does.
struct IssueCommand {
  let request: IssueRequest
  var output = Console.standardOutput

  func execute() -> ExitStatus {
    do {
      let (project, dir) = try locate()
      guard var report = try draftBugReport(project: project, dir: dir, secrets: StoredSecrets()) else {
        Console.standardError.line("jevision: " + words("cli.issue.notFailed", dir))
        return .failure
      }
      if let title = request.title { report.title = title }
      guard request.send else {
        preview(report, dir: dir)
        return .ok
      }
      return try send(report, project: project, dir: dir)
    } catch let error as UsageError {
      Console.standardError.line("jevision: \(error.message)")
    } catch let error as GitError {
      Console.standardError.line("jevision: \(Self.message(error))")
    } catch {
      Console.standardError.line("jevision: \(describe(error))")
    }
    return .failure
  }

  private func preview(_ report: BugReport, dir: String) {
    output.line("# \(report.title)")
    output.line()
    output.line(report.body)
    if !report.screenshots.isEmpty {
      output.line()
      output.line(words("cli.issue.screenshots"))
      for shot in report.screenshots { output.line("  \(caption(shot.label)): \(shot.path)") }
    }
    output.line()
    output.line(words("cli.issue.nothingSent", dir))
  }

  private func caption(_ label: String) -> String {
    switch label {
    case "Expected": words("bug.picture.expected")
    case "Actual": words("bug.picture.actual")
    case "Difference": words("bug.picture.difference")
    default: label
    }
  }

  private func send(_ report: BugReport, project: String, dir: String) throws -> ExitStatus {
    if request.commentOn == nil, !request.newIssue,
      let open = try githubOpenBugReport(project: project, fingerprint: report.fingerprint)
    {
      Console.standardError.line("jevision: " + words("cli.issue.alreadyOpen", open.number, open.title, open.url))
      Console.standardError.line(words("cli.issue.commentOrNew", open.number))
      return .problems
    }
    let url = try githubSendBugReport(
      project: project, dir: dir, report: report, commentOn: request.commentOn, secrets: StoredSecrets())
    output.line(url)
    return .ok
  }

  /// The project folder and the folder of the run to report, as real paths.
  private func locate() throws -> (project: String, dir: String) {
    let target = try request.target.map { path in
      guard let real = Self.realPath(path) else { throw UsageError(message: words("cli.error.noFileOrFolder", path)) }
      return real
    }
    let project: String
    if let given = request.project {
      guard let real = Self.realPath(given), isFolder(real) else {
        throw UsageError(message: words("cli.error.noFolder", given))
      }
      project = real
    } else if let found = MCPCommand.defaultProject(
      from: target.map { isFolder($0) ? $0 : ($0 as NSString).deletingLastPathComponent }
        ?? FileManager.default.currentDirectoryPath)
    {
      project = found
    } else {
      throw UsageError(message: words("cli.error.nameProject"))
    }
    if let target, FileManager.default.fileExists(atPath: target + "/report.json") {
      return (project, target)
    }
    if let target, isFolder(target) {
      throw UsageError(message: words("cli.error.notTestOrRun", target))
    }
    let file = target.map { target -> String in
      target.hasPrefix(project + "/") ? String(target.dropFirst(project.count + 1)) : target
    }
    let failed = try listRuns(project: project).first {
      $0.report.verdict == .failed && (file == nil || $0.report.file == file)
    }
    guard let failed else {
      throw UsageError(message: file.map { words("cli.issue.noFailedRunOf", $0) } ?? words("cli.issue.noFailedRun", project))
    }
    return (project, failed.dir)
  }

  private static func realPath(_ path: String) -> String? {
    guard let resolved = realpath(path, nil) else { return nil }
    defer { free(resolved) }
    return String(cString: resolved)
  }

  static func message(_ error: GitError) -> String {
    switch error {
    case .NoGitHubCli, .GitHubSignedOut, .SignIn:
      words("cli.issue.signedOut")
    case .NotRepository, .NoCommits, .NoRemote, .NotGitHub:
      words("cli.issue.notOnGitHub")
    case .NoPermission:
      words("cli.issue.noPermission")
    case .Unreachable:
      words("cli.issue.unreachable")
    case .EmptyMessage:
      words("cli.issue.emptyTitle")
    case .Failed(let message):
      message
    default:
      words("cli.issue.notTaken", "\(error)")
    }
  }
}
