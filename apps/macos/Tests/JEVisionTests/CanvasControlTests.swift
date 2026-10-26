import AppKit
import JEVisionHands
import SwiftUI
import Testing

@testable import JEVision

/// Exercises the editor in an offscreen window, without sending input to the desktop.
@MainActor
@Suite(.enabled(if: ProcessInfo.processInfo.environment["JEVISION_SCALE"] == "1"), .serialized)
struct CanvasControlTests {
  @Test func hidingAndShowingTheMapUpdatesWithoutAnotherCanvasAction() async throws {
    try await checkMinimap(board: false)
  }
}

@MainActor
func checkMinimap(board: Bool) async throws {
  let root = FileManager.default.temporaryDirectory.appending(path: "jevision-map-control-\(UUID().uuidString)")
  let profileFolder = root.appending(path: "Profile")
  let projectFolder = root.appending(path: "Workspace")
  for folder in [profileFolder, projectFolder] {
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
  }
  // The key exists on disk before the store is created. A first in-process write hides
  // the managed-domain defect that this scenario must exercise.
  let settings = try PropertyListSerialization.data(
    fromPropertyList: [AppPreferences.formatKey: AppPreferences.format, "flowShowsMinimap": false, "boardShowsMinimap": false],
    format: .binary, options: 0)
  try settings.write(to: profileFolder.appending(path: "Settings.plist"), options: .atomic)
  let profile = Profile(folder: profileFolder)
  defer {
    profile.defaults.removePersistentDomain(forName: profile.defaultsDomain)
    try? FileManager.default.removeItem(at: root)
  }
  let file = projectFolder.appending(path: "Map.flow.json")
  try #"{"format":1,"name":"Map","app":"com.apple.TextEdit","start":"a","blocks":{"a":{"type":"pause","duration":"1s"}}}"#
    .write(to: file, atomically: true, encoding: .utf8)
  let project = ProjectStore(defaults: profile.defaults)
  project.open(projectFolder)
  try await eventuallyAsync("the disposable project opens", timeout: 3) { project.status == .ready }
  project.selection = file.lastPathComponent
  let document = try #require(project.document)
  try #require(document.flow != nil)
  project.boards.sync(tests: [file.lastPathComponent], rootTitle: project.name)
  let boardName = try #require(project.boards.allTests)
  let preference = board ? "boardShowsMinimap" : "flowShowsMinimap"
  if board {
    project.showBoard(boardName)
    try #require(!project.boards.scene.nodes.isEmpty)
  }
  // Exercise the real floating workspace/toolbar, not just an editor hosted in isolation.
  // Preferences live outside the project, so its watcher cannot cause an incidental redraw.
  let host = NSHostingView(rootView: ProjectView(recordFirstTest: {})
    .modifier(WorkspaceChrome())
    .environment(project)
    .environment(RunCenter(defaults: profile.defaults))
    .environment(RecordCenter())
    .appPreferences(profile.defaults))
  let window = NSWindow(contentRect: NSRect(x: -10000, y: -10000, width: 1000, height: 700),
    styleMask: [.titled, .resizable], backing: .buffered, defer: false)
  window.isReleasedWhenClosed = false
  window.contentView = host
  window.orderBack(nil)
  defer {
    window.contentView = nil
    window.close()
  }

  try await Task.sleep(for: .milliseconds(300))
  func buttons(_ view: NSView) -> [NSButton] {
    ((view as? NSButton).map { [$0] } ?? []) + view.subviews.flatMap(buttons)
  }
  func mapButton() -> NSButton? {
    // SwiftUI exposes the identifier on a virtual AX element, not its backing NSButton.
    // In this unselected fixture the native minimap toggle is the rightmost canvas control.
    buttons(host).max { a, b in
      a.convert(a.bounds, to: host).midX < b.convert(b.bounds, to: host).midX
    }
  }
  func changed(from old: NSButton, shown: Bool) async throws -> NSButton {
    let oldY = old.convert(old.bounds, to: host).midY
    old.performClick(nil)
    try await eventuallyAsync("the map changes without layout, focus or camera actions", timeout: 3) {
      guard let button = mapButton(), button !== old else { return false }
      return abs(button.convert(button.bounds, to: host).midY - oldY) > 80
    }
    #expect(profile.defaults.bool(forKey: preference) == shown)
    return try #require(mapButton())
  }
  var button = try #require(mapButton())
  button = try await changed(from: button, shown: true)
  button = try await changed(from: button, shown: false)
  button = try await changed(from: button, shown: true)
  let shownY = button.convert(button.bounds, to: host).midY
  let shownButton = button
  // Another settings view can write the same preference; the canvas must observe it too.
  profile.defaults.set(false, forKey: preference)
  try await eventuallyAsync("external preference changes update the map", timeout: 3) {
    guard let button = mapButton(), button !== shownButton else { return false }
    return abs(button.convert(button.bounds, to: host).midY - shownY) > 80
  }
}
