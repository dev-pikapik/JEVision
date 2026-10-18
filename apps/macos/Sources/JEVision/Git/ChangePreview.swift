import JEVisionCore
import SwiftUI

/// What changed in one file between two versions: a test on its flowchart, a picture compared
/// with its older self.
struct ChangePreview: View {
  @Environment(ProjectStore.self) private var project
  let change: FileChange
  /// The commit with the older version; `nil` when there is none.
  let before: String?
  /// The commit with the newer version; `nil` for the file as it is on disk now.
  let after: String?

  private struct Versions: Hashable {
    var before: Data?
    var after: Data?
  }

  @State private var versions: Versions?

  var body: some View {
    Group {
      if change.kind == .conflicted {
        ContentUnavailableView(
          GitText.fileName(change.path), systemImage: GitText.symbol(change.kind),
          description: Text("This file has changes from two places that git couldn’t combine."))
      } else if let versions {
        if GitText.isTest(change.path) {
          let old = flow(versions.before)
          let new = flow(versions.after)
          if versions.before != nil && old == nil || versions.after != nil && new == nil {
            ContentUnavailableView(
              GitText.fileName(change.path), systemImage: "exclamationmark.triangle",
              description: Text("This version of the test can’t be opened, so it can’t be compared."))
          } else {
            FlowDiffView(before: old, after: new)
              .id(versions)
          }
        } else if GitText.isPicture(change.path) {
          PictureDiffView(before: versions.before, after: versions.after)
        } else {
          ContentUnavailableView(
            GitText.fileName(change.path), systemImage: GitText.symbol(change.kind),
            description: Text(GitText.title(change.kind)))
        }
      } else {
        ProgressView()
          .frame(maxWidth: .infinity, maxHeight: .infinity)
      }
    }
    .task(id: [change.path, change.from, before, after]) { await load() }
  }

  private func flow(_ data: Data?) -> Flow? {
    data.flatMap { String(data: $0, encoding: .utf8) }.flatMap { try? loadFlow(json: $0) }
  }

  private func load() async {
    guard let root = project.rootURL else { return }
    let path = root.path(percentEncoded: false)
    let change = change, before = before, after = after
    let found = await Task.detached(priority: .userInitiated) {
      var versions = Versions()
      if change.kind != .added, let before {
        versions.before = try? gitFile(project: path, commit: before, path: change.from ?? change.path)
      }
      if change.kind != .deleted {
        versions.after = if let after {
          try? gitFile(project: path, commit: after, path: change.path)
        } else {
          try? Data(contentsOf: root.appending(path: change.path))
        }
      }
      return versions
    }.value
    guard !Task.isCancelled else { return }
    versions = found
  }
}
