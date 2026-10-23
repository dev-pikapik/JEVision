import Foundation
import JEVisionHands
import Observation
import Sparkle

/// Updates of the app itself, through Sparkle. Only release builds carry the public key that
/// update archives are checked against; builds without it never look for updates. Neither does a
/// separate profile: Sparkle keeps its choices in the app's own settings, which are the person's,
/// and would ask them whether to look for updates.
@MainActor
@Observable
final class Updates: NSObject {
  /// The setting that also offers beta versions.
  static let betasKey = "updatesIncludeBetas"
  /// A feed a test copy of the app reads instead of the published ones. It is taken from the
  /// environment the app was opened with, never stored; docs/maintainers/releasing.md tells why
  /// that is safe.
  static let feedOverrideKey = "JEVISION_UPDATE_FEED"
  /// The Sparkle channel of beta versions in the beta feed.
  static let betaChannel = "beta"

  /// Whether this build can update itself.
  let isAvailable: Bool
  /// False while a check or an update is already in progress.
  private(set) var canCheck = false
  /// When JEVision last looked for an update, by itself or when asked; nil before the first time.
  private(set) var lastCheck: Date?
  @ObservationIgnored private let bundle: Bundle
  @ObservationIgnored private let defaults: UserDefaults
  @ObservationIgnored private let feedOverride: String?
  @ObservationIgnored private var controller: SPUStandardUpdaterController?
  @ObservationIgnored private var observation: NSKeyValueObservation?

  init(
    bundle: Bundle = .main, defaults: UserDefaults = Profile.current.defaults,
    environment: [String: String] = ProcessInfo.processInfo.environment
  ) {
    self.bundle = bundle
    self.defaults = defaults
    isAvailable = Self.hasPublicKey(bundle) && !Profile(environment: environment).isolated
    feedOverride = Self.feedOverride(in: environment)
    super.init()
    guard isAvailable else { return }
    if let feedOverride { AppLog.info("Updates come from \(feedOverride), set by \(Self.feedOverrideKey)", "updates") }
    let controller = SPUStandardUpdaterController(startingUpdater: true, updaterDelegate: self, userDriverDelegate: nil)
    self.controller = controller
    lastCheck = controller.updater.lastUpdateCheckDate
    observation = controller.updater.observe(\.canCheckForUpdates, options: [.initial, .new]) { [weak self] updater, _ in
      MainActor.assumeIsolated { self?.canCheck = updater.canCheckForUpdates }
    }
  }

  static func hasPublicKey(_ bundle: Bundle) -> Bool {
    let key = bundle.object(forInfoDictionaryKey: "SUPublicEDKey") as? String
    return !(key ?? "").trimmingCharacters(in: .whitespaces).isEmpty
  }

  /// The web address in `JEVISION_UPDATE_FEED`, when it is one.
  static func feedOverride(in environment: [String: String]) -> String? {
    guard let value = environment[feedOverrideKey], let url = URL(string: value), url.host() != nil,
      ["http", "https"].contains(url.scheme?.lowercased())
    else { return nil }
    return value
  }

  /// Whether Sparkle looks for updates on its own: when JEVision opens and once a day.
  var checksAutomatically: Bool {
    get {
      access(keyPath: \.checksAutomatically)
      return controller?.updater.automaticallyChecksForUpdates ?? false
    }
    set {
      withMutation(keyPath: \.checksAutomatically) { controller?.updater.automaticallyChecksForUpdates = newValue }
    }
  }

  /// Whether a found update is downloaded in the background and installed when JEVision quits.
  var downloadsAutomatically: Bool {
    get {
      access(keyPath: \.downloadsAutomatically)
      return controller?.updater.automaticallyDownloadsUpdates ?? false
    }
    set {
      withMutation(keyPath: \.downloadsAutomatically) { controller?.updater.automaticallyDownloadsUpdates = newValue }
    }
  }

  /// Whether beta versions are offered too, not only releases.
  var includesBetas: Bool {
    get {
      access(keyPath: \.includesBetas)
      return defaults.bool(forKey: Self.betasKey)
    }
    set {
      withMutation(keyPath: \.includesBetas) { defaults.set(newValue, forKey: Self.betasKey) }
    }
  }

  /// The feed to read: the beta one when betas are on, which lists the releases as well.
  var feedURL: String? {
    feedOverride ?? bundle.object(forInfoDictionaryKey: includesBetas ? "JEVisionBetaFeedURL" : "SUFeedURL") as? String
  }

  /// The channels besides the releases; the stable feed has no betas, this guards a mixed-up one.
  var channels: Set<String> { includesBetas ? [Self.betaChannel] : [] }

  /// Looks for an update now and shows what it finds, “up to date” included.
  func check() {
    controller?.checkForUpdates(nil)
  }
}

// Sparkle calls its delegate on the main thread.
extension Updates: SPUUpdaterDelegate {
  // Always an explicit feed, so a feed address left in the settings by hand is never used.
  func feedURLString(for updater: SPUUpdater) -> String? { feedURL }

  func allowedChannels(for updater: SPUUpdater) -> Set<String> { channels }

  // The last moment an update can still be stopped: after this Sparkle shows, downloads and
  // installs it. Not for a check that only reads the feed.
  func updater(_ updater: SPUUpdater, shouldProceedWithUpdate item: SUAppcastItem, updateCheck: SPUUpdateCheck) throws {
    guard updateCheck != .updateInformation else { return }
    let dataFormat = item.propertiesDictionary[UpdateDataGuard.feedKey] as? String
    do {
      if let backup = try UpdateDataGuard().prepare(version: item.displayVersionString, dataFormat: dataFormat) {
        AppLog.info("Saved a copy of the data before \(item.displayVersionString) in \(backup.path)", "updates")
      }
    } catch {
      AppLog.error("\(error.localizedDescription)", "updates")
      throw error
    }
  }

  func updater(_ updater: SPUUpdater, didFinishUpdateCycleFor updateCheck: SPUUpdateCheck, error: (any Error)?) {
    lastCheck = updater.lastUpdateCheckDate
  }
}
