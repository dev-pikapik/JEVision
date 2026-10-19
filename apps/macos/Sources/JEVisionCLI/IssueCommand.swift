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
        Console.standardError.line("jevision: the run in \(dir) didn’t fail, so there is no bug to report")
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
      output.line("Screenshots:")
      for shot in report.screenshots { output.line("  \(shot.label): \(shot.path)") }
    }
    output.line()
    output.line("Nothing is sent. File it on GitHub with: jevision issue \"\(dir)\" --send")
  }

  private func send(_ report: BugReport, project: String, dir: String) throws -> ExitStatus {
    if request.commentOn == nil, !request.newIssue,
      let open = try githubOpenBugReport(project: project, fingerprint: report.fingerprint)
    {
      Console.standardError.line(
        "jevision: issue #\(open.number) “\(open.title)” about this failure is open: \(open.url)")
      Console.standardError.line(
        "Add this run to it with --comment-on \(open.number), or open a new issue with --new.")
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
      guard let real = Self.realPath(path) else { throw UsageError(message: "there is no file or folder “\(path)”") }
      return real
    }
    let project: String
    if let given = request.project {
      guard let real = Self.realPath(given), isFolder(real) else {
        throw UsageError(message: "there is no folder “\(given)”")
      }
      project = real
    } else if let found = MCPCommand.defaultProject(
      from: target.map { isFolder($0) ? $0 : ($0 as NSString).deletingLastPathComponent }
        ?? FileManager.default.currentDirectoryPath)
    {
      project = found
    } else {
      throw UsageError(message: "name the project folder with --project")
    }
    if let target, FileManager.default.fileExists(atPath: target + "/report.json") {
      return (project, target)
    }
    if let target, isFolder(target) {
      throw UsageError(message: "“\(target)” is neither a test file nor the folder of a run")
    }
    let file = target.map { target -> String in
      target.hasPrefix(project + "/") ? String(target.dropFirst(project.count + 1)) : target
    }
    let failed = try listRuns(project: project).first {
      $0.report.verdict == .failed && (file == nil || $0.report.file == file)
    }
    guard let failed else {
      throw UsageError(message: file.map { "no run of \($0) has failed" } ?? "no run in \(project) has failed")
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
      "GitHub isn’t connected: sign in to GitHub in JEVision (Settings → Git), or set GH_TOKEN"
    case .NotRepository, .NoCommits, .NoRemote, .NotGitHub:
      "the project isn’t on GitHub, so the bug can’t be filed there; print the report without --send to file it elsewhere"
    case .NoPermission:
      "this GitHub account can’t open issues in the project’s repository"
    case .Unreachable:
      "GitHub can’t be reached; check the internet connection"
    case .EmptyMessage:
      "the title is empty"
    case .Failed(let message):
      message
    default:
      "GitHub didn’t take the bug report (\(error))"
    }
  }
}
