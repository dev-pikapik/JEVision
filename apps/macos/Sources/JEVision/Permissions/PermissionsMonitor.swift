import AppKit
import Observation

/// Current state of the permissions. Windows that show it run `watch()` in a `.task`,
/// so it checks every 1.5 seconds only while they are open.
@MainActor
@Observable
final class PermissionsMonitor {
  private(set) var granted: Set<Permission> = []

  var missing: [Permission] { Permission.allCases.filter { !granted.contains($0) } }
  var allGranted: Bool { missing.isEmpty }

  init() {
    refresh()
  }

  func refresh() {
    let now = Set(Permission.allCases.filter(\.isGranted))
    if now != granted { granted = now }
  }

  func watch() async {
    while !Task.isCancelled {
      refresh()
      try? await Task.sleep(for: .seconds(1.5))
    }
  }
}
