import Foundation
import Observation
import Sparkle

/// Updates of the app itself, through Sparkle. Only release builds carry the public key that
/// update archives are checked against; builds without it never look for updates.
@MainActor
@Observable
final class Updates {
  /// Whether this build can update itself.
  let isAvailable: Bool
  /// False while a check or an update is already in progress.
  private(set) var canCheck = false
  @ObservationIgnored private let controller: SPUStandardUpdaterController?
  @ObservationIgnored private var observation: NSKeyValueObservation?

  init(bundle: Bundle = .main) {
    isAvailable = Self.hasPublicKey(bundle)
    guard isAvailable else {
      controller = nil
      return
    }
    let controller = SPUStandardUpdaterController(startingUpdater: true, updaterDelegate: nil, userDriverDelegate: nil)
    self.controller = controller
    observation = controller.updater.observe(\.canCheckForUpdates, options: [.initial, .new]) { [weak self] updater, _ in
      MainActor.assumeIsolated { self?.canCheck = updater.canCheckForUpdates }
    }
  }

  static func hasPublicKey(_ bundle: Bundle) -> Bool {
    let key = bundle.object(forInfoDictionaryKey: "SUPublicEDKey") as? String
    return !(key ?? "").trimmingCharacters(in: .whitespaces).isEmpty
  }

  /// Whether Sparkle looks for updates on its own, once a day.
  var checksAutomatically: Bool {
    get {
      access(keyPath: \.checksAutomatically)
      return controller?.updater.automaticallyChecksForUpdates ?? false
    }
    set {
      withMutation(keyPath: \.checksAutomatically) { controller?.updater.automaticallyChecksForUpdates = newValue }
    }
  }

  /// Looks for an update now and shows what it finds, “up to date” included.
  func check() {
    controller?.checkForUpdates(nil)
  }
}
