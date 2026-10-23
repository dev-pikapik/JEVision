import JEVisionCore
import SwiftUI

/// What the editor shows for a test that can't be opened: why, and a way back to a version Git
/// keeps of it. Undo takes a restore back.
struct BrokenTestView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.undoManager) private var undoManager
  let document: FlowDocument
  let failure: String
  @State private var recovery: TestRecovery?
  @State private var restoreFailure: String?

  /// The test, relative to the project.
  private var path: String? {
    project.rootURL.map { RelativePath.from($0, to: document.url) }
  }

  var body: some View {
    ContentUnavailableView {
      Label("Can’t open this test", systemImage: "exclamationmark.triangle")
    } description: {
      VStack(spacing: 12) {
        Text(failure)
          .textSelection(.enabled)
        if let recovery {
          explanation(recovery)
            .foregroundStyle(.secondary)
        }
      }
      .frame(maxWidth: 480)
    } actions: {
      if case .version(let commit, let source) = recovery {
        Button("Restore Last Committed Version") { restore(source, from: commit) }
          .buttonStyle(.borderedProminent)
          .help(Text("Put the test back as it was in that commit. Edit › Undo brings back the file as it is now."))
      }
      if recovery?.isCommitted == true {
        Button("Browse All Versions…") { project.historyPath = path }
          .help(Text("Pick any committed version of the test to put back."))
      }
      Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([document.url]) }
    }
    .task(id: failure) {
      guard let root = project.rootURL, let path else { return }
      recovery = await Task.detached(priority: .userInitiated) { try? TestRecovery.find(path, in: root) }.value
    }
    // A version put back from the history shows at once, not when the folder is looked at again.
    .onChange(of: project.historyPath) { _, path in
      if path == nil { document.reload() }
    }
    .alert(
      "Couldn’t restore this version",
      isPresented: Binding(get: { restoreFailure != nil }, set: { if !$0 { restoreFailure = nil } }),
      presenting: restoreFailure
    ) { _ in
      Button("OK") {}
    } message: { Text($0) }
  }

  private func explanation(_ recovery: TestRecovery) -> Text {
    switch recovery {
    case .noGit:
      Text("This project isn’t in Git, so there’s no earlier version of the test to bring back.")
    case .notCommitted:
      Text("This test was never committed to Git, so there’s no earlier version of it to bring back.")
    case .nothingOpens:
      Text("None of the versions committed to Git open either, but you can still look through them.")
    case .version(let commit, _):
      Text(
        "The last committed version that opens is from “\(commit.summary)”, \(Date(timeIntervalSince1970: TimeInterval(commit.time)), format: .relative(presentation: .named))."
      )
    }
  }

  /// Puts the test back as `source` was in `commit`; the file now in its place goes to the Trash.
  private func restore(_ source: String, from commit: GitCommit) {
    guard let path else { return }
    do {
      // The file now in its place still goes to the Trash; Undo writes it back itself, so the
      // editor shows it again at once.
      try TestRecovery.restore(document.url, open: { [weak project] in project?.openDocuments ?? [] }, in: undoManager) {
        try project.restore(path, from: source, in: commit.id, undoManager: nil)
      }
    } catch {
      restoreFailure = GitText.failure(error)
    }
  }
}

/// What Git can do for a test that can't be opened.
enum TestRecovery: Equatable {
  /// Git isn't installed, or it doesn't keep the project.
  case noGit
  /// Git keeps the project, but the test was never committed.
  case notCommitted
  /// The test was committed, but none of its versions opens either.
  case nothingOpens
  /// The newest version that opens: the file as `source` was in `commit`.
  case version(GitCommit, source: String)

  var isCommitted: Bool {
    switch self {
    case .noGit, .notCommitted: false
    case .nothingOpens, .version: true
    }
  }

  /// Looks through the history of `test`, a path relative to `project`, newest first, for a
  /// version that opens. The newest commit may hold the broken file itself. Runs git, so it
  /// belongs off the main thread.
  static func find(_ test: String, in project: URL) throws -> TestRecovery {
    let root = project.path(percentEncoded: false)
    let history: [GitCommit]
    do {
      history = try gitHistory(project: root, path: test)
    } catch GitError.NotInstalled, GitError.NotRepository {
      return .noGit
    }
    for commit in history {
      guard let change = commit.files.first, change.kind != .deleted,
        let data = try gitFile(project: root, commit: commit.id, path: change.path),
        let text = String(data: data, encoding: .utf8), (try? loadFlow(json: text)) != nil
      else { continue }
      return .version(commit, source: change.path)
    }
    return history.isEmpty ? .notCommitted : .nothingOpens
  }

  /// Puts a version of `file` back by running `write`, which writes it, as one step for Undo.
  /// `open` gives the tests open for editing at the time: the one of `file`, if any, shows at once
  /// what the restore, and later Undo and Redo, leave in the file, so its next edit can't save over it.
  @MainActor
  static func restore(
    _ file: URL, open: @escaping () -> [FlowDocument], in undoManager: UndoManager?,
    ask: @escaping FileUndo.Ask = FileUndo.askToReplace, by write: () throws -> Void
  ) throws {
    let path = file.standardizedFileURL.path(percentEncoded: false)
    let show = {
      for document in open() where document.url.standardizedFileURL.path(percentEncoded: false) == path {
        document.reload()
      }
    }
    try FileUndo.record(file, action: String(localized: "Restore Version"), in: undoManager, ask: ask, then: show, write)
    show()
  }
}
