import AppKit

/// What the app does for the Finder and the Dock: opens the projects and tests dropped on its
/// icon or opened with it, and lists the recent projects in the Dock menu.
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
  var project: ProjectStore?
  /// Brings the main window back when it was closed; set by the window once it's shown.
  var showMainWindow: () -> Void = {}

  func application(_ application: NSApplication, open urls: [URL]) {
    guard let project else { return }
    guard urls.contains(where: { project.openItem($0) }) else {
      NSSound.beep()
      return
    }
    showMainWindow()
    NSApp.activate()
  }

  func applicationDockMenu(_ sender: NSApplication) -> NSMenu? {
    guard let project, !project.openableRecent.isEmpty else { return nil }
    let menu = NSMenu()
    for entry in project.openableRecent {
      let item = NSMenuItem(title: project.menuTitle(entry), action: #selector(openRecent(_:)), keyEquivalent: "")
      item.target = self
      item.representedObject = entry.path
      item.state = entry.path == project.rootURL?.path ? .on : .off
      menu.addItem(item)
    }
    return menu
  }

  @objc private func openRecent(_ sender: NSMenuItem) {
    guard let path = sender.representedObject as? String else { return }
    NSApp.activate()
    showMainWindow()
    project?.openRecent(path)
  }
}
