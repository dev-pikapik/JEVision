import JEVisionCore
import SwiftUI

/// The commits that changed a test or a folder. Picking one shows what it changed; any version
/// can be put back, and Undo takes that back.
struct HistoryView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  /// The test or folder, relative to the project.
  let path: String
  /// The main window's, so Edit › Undo there takes a restore back.
  let undoManager: UndoManager?

  @State private var commits: [GitCommit]?
  @State private var selection: GitCommit.ID?
  @State private var file: String?
  @State private var failure: String?

  private var isTest: Bool { GitText.isTest(path) }
  private var commit: GitCommit? { commits?.first { $0.id == selection } }
  private var change: FileChange? {
    guard let commit else { return nil }
    return isTest ? commit.files.first : commit.files.first { $0.path == file } ?? commit.files.first
  }

  var body: some View {
    VStack(spacing: 0) {
      Text("History of “\(GitText.fileName(path))”")
        .font(.headline)
        .padding(.vertical, 10)
        .accessibilityAddTraits(.isHeader)
      Divider()
      HSplitView {
        list
          .frame(minWidth: 240, idealWidth: 280, maxWidth: 360)
        detail
          .frame(minWidth: 420, maxWidth: .infinity, maxHeight: .infinity)
      }
      Divider()
      HStack {
        if let change, let commit {
          Button("Restore This Version") { restore(change, from: commit) }
            .disabled(change.kind == .deleted)
            .help(Text("Put the file back as it was after this commit. Edit › Undo takes it back."))
        }
        Spacer()
        Button("Done") { dismiss() }
          .keyboardShortcut(.defaultAction)
      }
      .padding()
    }
    .frame(minWidth: 760, idealWidth: 960, minHeight: 480, idealHeight: 640)
    .task { await load() }
    .alert(
      "Couldn’t restore this version", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } }),
      presenting: failure
    ) { _ in
      Button("OK") {}
    } message: { Text($0) }
  }

  @ViewBuilder private var list: some View {
    if let commits {
      if commits.isEmpty {
        ContentUnavailableView(
          "No history yet", systemImage: "clock.arrow.circlepath",
          description: isTest
            ? Text("Commit this test to keep its first version here.")
            : Text("Commit the tests in this folder to keep their versions here."))
      } else {
        List(commits, selection: $selection) { commit in
          VStack(alignment: .leading, spacing: 2) {
            Text(commit.summary)
              .font(.headline)
              .lineLimit(2)
            Text("\(commit.author) · \(Date(timeIntervalSince1970: TimeInterval(commit.time)), format: .relative(presentation: .named))")
              .foregroundStyle(.secondary)
            Text(verbatim: commit.shortId)
              .font(.caption.monospaced())
              .foregroundStyle(.tertiary)
              .accessibilityLabel(Text("Commit \(commit.shortId)"))
          }
          .padding(.vertical, 2)
          .accessibilityElement(children: .combine)
        }
      }
    } else {
      ProgressView()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
  }

  @ViewBuilder private var detail: some View {
    if let commit, let change {
      VStack(alignment: .leading, spacing: 0) {
        if !isTest, commit.files.count > 1 {
          Picker("File", selection: Binding(get: { change.path }, set: { file = $0 })) {
            ForEach(commit.files, id: \.path) { file in
              Label(GitText.fileName(file.path), systemImage: GitText.symbol(file.kind)).tag(file.path)
            }
          }
          .padding([.horizontal, .top])
        }
        ChangePreview(change: change, before: commit.parent, after: commit.id)
          .id([commit.id, change.path])
      }
    } else if commits?.isEmpty == false {
      ContentUnavailableView("Select a commit", systemImage: "clock.arrow.circlepath", description: Text("See what it changed."))
    } else {
      Color.clear
    }
  }

  private func load() async {
    guard let root = project.rootURL?.path(percentEncoded: false) else { return }
    let path = path
    let found = await Task.detached(priority: .userInitiated) { (try? gitHistory(project: root, path: path)) ?? [] }.value
    commits = found
    selection = found.first?.id
  }

  /// A test comes back under its current name; a folder's file under the name it had then.
  private func restore(_ change: FileChange, from commit: GitCommit) {
    let target = isTest ? path : change.path
    do {
      try project.restore(target, from: change.path, in: commit.id, undoManager: undoManager)
    } catch {
      failure = GitText.failure(error)
      return
    }
    if GitText.isTest(target) { project.selection = target }
    dismiss()
  }
}

extension GitCommit: @retroactive Identifiable {}
