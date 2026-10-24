import Foundation
import JEVisionCore
import Observation

/// A list row names its project as well as its file. Relative paths can repeat in other folders.
enum ProjectSidebarSelection: Hashable {
  case project(String)
  case item(project: String, file: String)
  case board(String)
  case run(String)
}

/// Read-only test trees for the project's folders in the sidebar. Expanding one never changes
/// the active project, creates boards, starts services or dismisses an editor.
@MainActor
@Observable
final class ProjectCatalog {
  struct Snapshot: Sendable {
    let items: [ProjectItem]
    let problem: String?
  }

  private(set) var snapshots: [String: Snapshot] = [:]
  @ObservationIgnored private var tasks: [String: Task<Void, Never>] = [:]
  @ObservationIgnored private var previousRead: Task<Void, Never>?
  @ObservationIgnored private var caches: [String: ProjectScanner.Cache] = [:]

  func load(_ entry: RecentProject, refresh: Bool = false) {
    guard tasks[entry.path] == nil, refresh || snapshots[entry.path] == nil else { return }
    let cache = caches[entry.path] ?? ProjectScanner.Cache()
    caches[entry.path] = cache
    let previous = previousRead
    let work = Task { [weak self] in
      // Searching a large library must not start one filesystem scan per project at once.
      await previous?.value
      guard !Task.isCancelled else { return }
      let result = await Task.detached(priority: .utility) {
        guard let folder = ProjectStore.folder(of: entry) else {
          return Snapshot(items: [], problem: String(localized: "The folder was moved, deleted or is on a disk that isn’t connected."))
        }
        let accessed = folder.startAccessingSecurityScopedResource()
        defer { if accessed { folder.stopAccessingSecurityScopedResource() } }
        do { return Snapshot(items: try ProjectScanner.scan(folder, cache: cache).items, problem: nil) }
        catch { return Snapshot(items: [], problem: error.localizedDescription) }
      }.value
      guard let self, !Task.isCancelled else { return }
      snapshots[entry.path] = result
      tasks[entry.path] = nil
    }
    tasks[entry.path] = work
    previousRead = work
  }

  func wait(for path: String) async { await tasks[path]?.value }
}
