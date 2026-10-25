import Foundation
import JEVisionCore

/// Where the app and the `jevision` tool keep what belongs to the person: settings, logs, keys and
/// files such as the AI journal. `JEVISION_PROFILE=<folder>` keeps all of it in that folder instead,
/// a separate profile, so a second copy of JEVision, such as the one JEVision's own tests drive,
/// leaves the person's data alone. What a profile can't keep to itself is off in one: schedules,
/// installing the command line tool, updates and notifications.
public final class Profile: @unchecked Sendable {
  public static let variable = "JEVISION_PROFILE"

  /// The profile of this process, from its environment.
  public static let current = Profile(environment: ProcessInfo.processInfo.environment)

  /// The profile's folder; `nil` for the person's own data.
  public let folder: URL?
  /// JEVision's folder in Application Support, or `Support` in the profile.
  public let supportFolder: URL
  /// `~/Library/Logs/JEVision`, or `Logs` in the profile.
  public let logsFolder: URL
  /// The settings of this process: its standard ones, or the profile's. The app's views and
  /// stores keep theirs here. Not marked Sendable by the SDK, though safe to use from any thread.
  public let defaults: UserDefaults
  /// The name of the domain `defaults` keeps its settings in.
  public let defaultsDomain: String
  /// The app's settings, which the `jevision` tool reads too: from the app itself they are
  /// `defaults`.
  public let appDefaults: UserDefaults
  /// The name of the domain `appDefaults` keeps its settings in, to pick up the app's changes.
  public let appDefaultsDomain: String

  private let path: String?

  /// Whether this is a separate profile rather than the person's own data.
  public var isolated: Bool { folder != nil }

  /// The profile `environment` names; a relative folder is in `current`.
  public convenience init(
    environment: [String: String], current: String = FileManager.default.currentDirectoryPath,
    bundleIdentifier: String? = Bundle.main.bundleIdentifier
  ) {
    let folder = profileFolder(value: environment[Self.variable] ?? "", current: current)
    self.init(folder: folder.map { URL(filePath: $0, directoryHint: .isDirectory) }, bundleIdentifier: bundleIdentifier)
  }

  /// `folder` is the profile's, `nil` for the person's own data; `bundleIdentifier` tells the app
  /// from the `jevision` tool.
  public init(folder: URL?, bundleIdentifier: String? = Bundle.main.bundleIdentifier) {
    // Written the way the core writes it, which names the profile's keys the same way.
    path = folder.flatMap { profileFolder(value: $0.path(percentEncoded: false), current: "/") }
    self.folder = path.map { URL(filePath: $0, directoryHint: .isDirectory) }
    if let folder = self.folder {
      supportFolder = folder.appending(path: "Support", directoryHint: .isDirectory)
      logsFolder = folder.appending(path: "Logs", directoryHint: .isDirectory)
      // A suite named by an absolute path keeps its settings in that file, `Settings.plist`,
      // and every process that names it shares them, as the app's own domain is shared.
      let domain = folder.appending(path: "Settings").path(percentEncoded: false)
      // A path is never one of the names a suite can't have.
      let settings = UserDefaults(suiteName: domain)!
      defaults = settings
      defaultsDomain = domain
      appDefaults = settings
      appDefaultsDomain = domain
    } else {
      supportFolder = URL.applicationSupportDirectory.appending(path: "JEVision", directoryHint: .isDirectory)
      logsFolder = URL.libraryDirectory.appending(path: "Logs/JEVision", directoryHint: .isDirectory)
      defaults = .standard
      defaultsDomain = bundleIdentifier ?? ""
      appDefaults =
        bundleIdentifier == AISettings.appDomain ? .standard : UserDefaults(suiteName: AISettings.appDomain) ?? .standard
      appDefaultsDomain = AISettings.appDomain
    }
    let backups = supportFolder.appending(path: "Preferences Backups", directoryHint: .isDirectory)
    if !defaultsDomain.isEmpty {
      AppPreferences.register(defaults, domain: defaultsDomain,
        file: Self.preferencesFile(domain: defaultsDomain), backups: backups)
    }
    AppPreferences.register(appDefaults, domain: appDefaultsDomain,
      file: Self.preferencesFile(domain: appDefaultsDomain), backups: backups)
  }

  private static func preferencesFile(domain: String) -> URL {
    if domain.hasPrefix("/") { return URL(filePath: domain + ".plist") }
    return FileManager.default.homeDirectoryForCurrentUser.appending(path: "Library/Preferences/" + domain + ".plist")
  }

  /// The Keychain service that stands for `service` here: a profile keeps its keys apart from the
  /// person's, and never reads theirs.
  public func keychainService(_ service: String) -> String {
    profileKeychainService(service: service, profile: path)
  }
}
