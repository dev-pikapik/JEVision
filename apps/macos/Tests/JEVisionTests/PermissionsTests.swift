import Foundation
import Network
import Testing
import UserNotifications

@testable import JEVision

/// A newcomer must see no macOS prompt before pressing a button that asks for one: the app only
/// checks permissions with calls that never prompt, asks in `Permission.request()` alone, and
/// leaves taking pictures of the screen to the runner.
struct PermissionsTests {
  private static let app = URL(filePath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .appending(path: "Sources/JEVision")

  private static func sources() throws -> [(name: String, text: String)] {
    let files = FileManager.default.enumerator(at: app, includingPropertiesForKeys: nil)?
      .compactMap { $0 as? URL }.filter { $0.pathExtension == "swift" } ?? []
    return try files.map { ($0.path.replacingOccurrences(of: app.path + "/", with: ""), try String(contentsOf: $0, encoding: .utf8)) }
  }

  @Test func onlyTheAllowButtonAsksMacOS() throws {
    let prompting = ["CGRequestScreenCaptureAccess", "AXIsProcessTrustedWithOptions", "tccutil"]
    let askers = try Self.sources().filter { file in prompting.contains { file.text.contains($0) } }.map(\.name)
    #expect(askers == ["Permissions/Permission.swift"])
  }

  @Test func onlyOptionalPermissionsBrowseTheLocalNetwork() throws {
    // A browser makes macOS ask for the local network, so only the row that says so may start one.
    let browsers = try Self.sources().filter { $0.text.contains("NWBrowser") }.map(\.name)
    #expect(browsers == ["Permissions/OptionalPermissions.swift"])
  }

  @Test func theAppNeverCapturesTheScreenItself() throws {
    let capturing = ["SCShareableContent", "SCScreenshotManager", "SCStream", "CGWindowListCreateImage", "CGDisplayCreateImage"]
    let capturers = try Self.sources().filter { file in capturing.contains { file.text.contains($0) } }.map(\.name)
    #expect(capturers.isEmpty)
  }
}

struct OptionalPermissionsTests {
  @Test func readsTheNotificationAnswerOfMacOS() {
    #expect(OptionalStatus(UNAuthorizationStatus.notDetermined) == .notAsked)
    #expect(OptionalStatus(UNAuthorizationStatus.denied) == .denied)
    #expect(OptionalStatus(UNAuthorizationStatus.authorized) == .allowed)
    #expect(OptionalStatus(UNAuthorizationStatus.provisional) == .allowed)
  }

  @Test func readsTheLocalNetworkFromWhatTheBrowserDoes() {
    #expect(OptionalStatus(NWBrowser.State.ready) == .allowed)
    #expect(OptionalStatus(NWBrowser.State.waiting(.dns(-65570))) == .denied)
    // Another reason to wait says nothing about the permission.
    #expect(OptionalStatus(NWBrowser.State.waiting(.dns(-65555))) == nil)
    #expect(OptionalStatus(NWBrowser.State.setup) == nil)
    #expect(OptionalStatus(NWBrowser.State.cancelled) == nil)
  }

  @MainActor @Test func leavesTheLocalNetworkAloneUntilThePersonAsked() throws {
    let name = "OptionalPermissionsTests.\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: name))
    defer { defaults.removePersistentDomain(forName: name) }
    let permissions = OptionalPermissions(defaults: defaults)
    #expect(permissions.status(of: .localNetwork) == .unknown)
    permissions.refresh()
    #expect(permissions.status(of: .localNetwork) == .notAsked)
  }

  @Test func theSectionsRowsAreFoundBySearch() {
    let ids = Set(SettingsItem.all.map(\.id))
    for permission in OptionalPermission.allCases {
      #expect(ids.contains("optionalPermission.\(permission.id)"))
    }
  }
}
