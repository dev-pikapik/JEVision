import AppKit
import JEVisionHands
import Network
import Observation
import SwiftUI
import UserNotifications

/// A permission only some features need. JEVision works without it, so it is kept apart from
/// `Permission`, whose missing members stop a test from running and raise the warning sign.
enum OptionalPermission: CaseIterable, Identifiable {
  case notifications, localNetwork

  var id: Self { self }

  var title: String {
    switch self {
    case .notifications: String(localized: "Notifications")
    case .localNetwork: String(localized: "Local Network")
    }
  }

  var reason: String {
    switch self {
    case .notifications: String(localized: "To tell you when a scheduled test fails or is skipped")
    case .localNetwork: String(localized: "To let the iPhone app find this Mac on the same Wi-Fi")
    }
  }

  /// Why the permission isn't asked for in this copy of the app, if it isn't.
  var unavailableReason: String? {
    self == .notifications && Profile.current.isolated
      ? String(localized: "Off in a separate profile, which runs no schedules.") : nil
  }

  var symbol: String {
    switch self {
    case .notifications: "bell.badge.fill"
    case .localNetwork: "wifi"
    }
  }

  var color: Color {
    switch self {
    case .notifications: .red
    case .localNetwork: .teal
    }
  }

  /// The list of System Settings where the person can change it.
  func openSettings() {
    let address =
      switch self {
      case .notifications:
        "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=\(Bundle.main.bundleIdentifier ?? "")"
      case .localNetwork: "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork"
      }
    guard let url = URL(string: address) else { return }
    NSWorkspace.shared.open(url)
  }
}

/// Where an optional permission stands.
enum OptionalStatus: Equatable {
  /// Not looked at yet, or waiting for macOS to answer.
  case unknown
  /// The person wasn't asked; asking is harmless.
  case notAsked
  case allowed
  case denied

  /// The way macOS reports the notification permission.
  init(_ status: UNAuthorizationStatus) {
    switch status {
    case .notDetermined: self = .notAsked
    case .denied: self = .denied
    case .authorized, .provisional, .ephemeral: self = .allowed
    @unknown default: self = .unknown
    }
  }

  /// The way macOS reports the local network permission, which has no call to ask about it: a
  /// browser for the service JEVision declares gets ready when allowed and waits with a refusal
  /// when not. `nil` while it hasn't decided.
  init?(_ state: NWBrowser.State) {
    switch state {
    case .ready:
      self = .allowed
    case .waiting(.dns(let code)) where code == Self.policyDenied:
      self = .denied
    default:
      return nil
    }
  }

  /// `kDNSServiceErr_PolicyDenied`, what a browser says when the local network is off for the app.
  private static let policyDenied: Int32 = -65570
}

/// The state of the optional permissions. The local network can be looked at only by browsing,
/// which makes macOS ask the first time, so that waits until the person pressed Allow once.
@MainActor
@Observable
final class OptionalPermissions {
  private(set) var statuses: [OptionalPermission: OptionalStatus] = [:]

  private static let askedKey = "optionalPermissionAsked.localNetwork"
  /// The Bonjour service the Mac declares in its Info.plist.
  private static let service = "_irohv1._udp"

  private let defaults: PreferenceStore
  @ObservationIgnored private var browser: NWBrowser?

  init(defaults: any PreferenceSource = Profile.current.defaults) {
    let defaults = defaults.preferenceStore
    self.defaults = defaults
  }

  func status(of permission: OptionalPermission) -> OptionalStatus { statuses[permission] ?? .unknown }

  func refresh() {
    Task { statuses[.notifications] = await Self.notificationStatus() }
    if defaults.bool(forKey: Self.askedKey) {
      probeLocalNetwork()
    } else {
      statuses[.localNetwork] = .notAsked
    }
  }

  /// Asks macOS; once it was refused the answer is only in System Settings.
  func allow(_ permission: OptionalPermission) {
    guard AppPreferences.write(in: defaults, {}) else { return }
    switch permission {
    case .notifications:
      guard ScheduleNotifier.canNotify else { return }
      UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in
        Task { @MainActor in self.refresh() }
      }
    case .localNetwork:
      AppPreferences.set(true, forKey: Self.askedKey, in: defaults)
      probeLocalNetwork()
    }
  }

  func stop() {
    browser?.cancel()
    browser = nil
  }

  private static func notificationStatus() async -> OptionalStatus {
    // Notifications work only in the app itself, not in tests or `swift run`.
    guard ScheduleNotifier.canNotify else { return .notAsked }
    return OptionalStatus(await UNUserNotificationCenter.current().notificationSettings().authorizationStatus)
  }

  /// Browses until macOS has answered, then stops: a later look starts again.
  private func probeLocalNetwork() {
    stop()
    let browser = NWBrowser(for: .bonjour(type: Self.service, domain: nil), using: .udp)
    browser.stateUpdateHandler = { [weak self] state in
      guard let status = OptionalStatus(state) else { return }
      // The browser runs on the main queue.
      MainActor.assumeIsolated {
        self?.statuses[.localNetwork] = status
        self?.stop()
      }
    }
    self.browser = browser
    statuses[.localNetwork] = statuses[.localNetwork] ?? .unknown
    browser.start(queue: .main)
  }
}

/// The permissions only some features need, under the two JEVision always needs. Lives inside a
/// grouped `Form`.
struct OptionalPermissionsSection: View {
  @State private var permissions = OptionalPermissions()

  var body: some View {
    Section {
      ForEach(OptionalPermission.allCases) { OptionalPermissionRow(permission: $0, permissions: permissions) }
    } header: {
      Text("For some features")
    } footer: {
      Text("These are up to you: without them JEVision works, only the feature named next to each doesn’t.")
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
    }
    .task { permissions.refresh() }
    .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
      permissions.refresh()
    }
    .onDisappear { permissions.stop() }
  }
}

private struct OptionalPermissionRow: View {
  let permission: OptionalPermission
  let permissions: OptionalPermissions

  var body: some View {
    let status = permissions.status(of: permission)
    LabeledContent {
      if let reason = permission.unavailableReason {
        Text(reason)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      } else {
        controls(status)
      }
    } label: {
      HStack(spacing: 10) {
        IconTile(symbol: permission.symbol, color: permission.color, size: 30)
        VStack(alignment: .leading, spacing: 2) {
          Text(permission.title)
          Text(permission.reason)
            .font(.subheadline)
            .foregroundStyle(.secondary)
        }
        .fixedSize(horizontal: false, vertical: true)
      }
    }
    .settingAnchor("optionalPermission.\(permission.id)")
  }

  private func controls(_ status: OptionalStatus) -> some View {
    HStack(spacing: 10) {
      Group {
        switch status {
        case .notAsked:
          Button("Allow", systemImage: "checkmark.circle") { permissions.allow(permission) }
            .buttonStyle(.glassProminent)
        case .denied:
          Button("Open Settings", systemImage: "arrow.up.forward.app", action: permission.openSettings)
            .buttonStyle(.glassProminent)
            .help(Text("Opens this list in System Settings"))
        case .unknown:
          Button("Open Settings", systemImage: "arrow.up.forward.app", action: permission.openSettings)
            .buttonStyle(.glass)
            .help(Text("Opens this list in System Settings"))
        case .allowed:
          Button("Turn Off", systemImage: "arrow.up.forward.app", action: permission.openSettings)
            .buttonStyle(.glass)
            .help(Text("Opens this list in System Settings"))
        }
      }
      .labelStyle(.titleAndIcon)
      Image(systemName: Self.symbol(status))
        .font(.title2)
        .foregroundStyle(status == .allowed ? AnyShapeStyle(.green) : AnyShapeStyle(.secondary))
        .contentTransition(.symbolEffect(.replace))
        .accessibilityLabel(Text(Self.title(status)))
    }
  }

  /// Shape and colour tell the state apart; an unfinished question is a dotted circle.
  static func symbol(_ status: OptionalStatus) -> String {
    switch status {
    case .allowed: "checkmark.circle.fill"
    case .denied: "xmark.circle.fill"
    case .notAsked: "questionmark.circle"
    case .unknown: "circle.dotted"
    }
  }

  static func title(_ status: OptionalStatus) -> String {
    switch status {
    case .allowed: String(localized: "Allowed")
    case .denied: String(localized: "Not allowed")
    case .notAsked: String(localized: "Not asked yet")
    case .unknown: String(localized: "Checking…")
    }
  }
}
