import AppKit
import Observation
import Security

/// Current state of the permissions. Windows that show it run `watch()` in a `.task`,
/// so it checks every 1.5 seconds only while they are open; it also checks whenever JEVision
/// comes to the front, as it does on the way back from System Settings.
@MainActor
@Observable
final class PermissionsMonitor {
  private(set) var granted: Set<Permission> = []
  /// Missing permissions whose switch in System Settings is probably on but doesn't work:
  /// macOS remembers an older copy of JEVision, or the person allowed it and it still fails.
  private(set) var stale: Set<Permission> = []
  /// Missing permissions that were given to a differently signed copy of JEVision.
  private(set) var olderCopy: Set<Permission> = []

  var missing: [Permission] { Permission.allCases.filter { !granted.contains($0) } }
  var allGranted: Bool { missing.isEmpty }

  private let defaults: UserDefaults
  @ObservationIgnored private var observer: NSObjectProtocol?
  /// Asked for since JEVision last came to the front: the person is still in System Settings.
  @ObservationIgnored private var away: Set<Permission> = []

  init(defaults: UserDefaults = .standard) {
    self.defaults = defaults
    refresh()
    observer = NotificationCenter.default.addObserver(
      forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main
    ) { [weak self] _ in
      MainActor.assumeIsolated {
        self?.away = []
        self?.refresh()
      }
    }
  }

  func refresh() {
    let now = Set(Permission.allCases.filter(\.isGranted))
    if now != granted { granted = now }
    let me = Self.signature
    for permission in now {
      defaults.set(me, forKey: Self.grantedKey(permission))
      defaults.removeObject(forKey: Self.requestedKey(permission))
    }
    let older = Set(
      missing.filter { permission in
        defaults.string(forKey: Self.grantedKey(permission)).map { $0 != me } ?? false
      })
    let asked = Set(missing.filter { defaults.bool(forKey: Self.requestedKey($0)) && !away.contains($0) })
    if older != olderCopy { olderCopy = older }
    if older.union(asked) != stale { stale = older.union(asked) }
  }

  /// Asks for `permission` and remembers that it was asked for, so coming back without it
  /// offers a reset.
  func request(_ permission: Permission) {
    defaults.set(true, forKey: Self.requestedKey(permission))
    away.insert(permission)
    permission.request()
  }

  func reset(_ permission: Permission) async {
    defaults.set(true, forKey: Self.requestedKey(permission))
    away.insert(permission)
    await permission.reset()
    refresh()
  }

  func watch() async {
    while !Task.isCancelled {
      refresh()
      try? await Task.sleep(for: .seconds(1.5))
    }
  }

  private static func grantedKey(_ permission: Permission) -> String { "permissionGrantedTo.\(permission.service)" }
  private static func requestedKey(_ permission: Permission) -> String { "permissionRequested.\(permission.service)" }

  /// Who macOS gives a permission to: this copy's designated requirement, which changes when
  /// JEVision is signed by someone else or, unsigned, rebuilt.
  static let signature: String = {
    var code: SecCode?
    var staticCode: SecStaticCode?
    var requirement: SecRequirement?
    var text: CFString?
    guard SecCodeCopySelf([], &code) == errSecSuccess, let code,
      SecCodeCopyStaticCode(code, [], &staticCode) == errSecSuccess, let staticCode,
      SecCodeCopyDesignatedRequirement(staticCode, [], &requirement) == errSecSuccess, let requirement,
      SecRequirementCopyString(requirement, [], &text) == errSecSuccess, let text
    else { return Bundle.main.bundlePath }
    return text as String
  }()
}
