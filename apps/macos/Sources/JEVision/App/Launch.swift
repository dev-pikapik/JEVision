import AppKit
import JEVisionCore
import JEVisionHands

/// Starts JEVision: the app, or with `--serve-cli <folder>` the host of a `jevision run
/// --through-app`, which runs the command in the app's name, without windows or a Dock icon.
@main
enum Launch {
  @MainActor static func main() {
    let arguments = CommandLine.arguments
    if let index = arguments.firstIndex(of: CLIBridge.serveArgument), arguments.indices.contains(index + 1) {
      NSApplication.shared.setActivationPolicy(.prohibited)
      let folder = URL(filePath: arguments[index + 1], directoryHint: .isDirectory)
      exit(CLIBridge.serve(folder, helper: CommandLineTool.bundled.tool))
    }
    // A rejected preferences file cannot supply the error dialog's language.
    setLanguage(tag: AppLanguage.resolve(saved: "", system: AppLanguage.systemLanguages(), available: AppLanguage.languages(in: .main)))
    do {
      try startGUI()
    } catch {
      showPreferencesFailure(error)
      exit((error as? AppPreferences.Failure)?.status ?? 3)
    }
  }

  /// The application closure is deliberately deferred until compatibility and legacy backup
  /// complete: even the App's stored-property initializers must not run before this gate.
  @MainActor static func startGUI(_ profile: Profile = .current, application: () -> Void = { JEVisionApp.main() }) throws {
    try AppPreferences.prepareForGUI(profile)
    if profile.isolated { isolate(profile) }
    watchPreferencesFailures()
    application()
  }

  /// Installed before the app initializes: a refused binding or background write produces the
  /// same visible explanation. Repeated timer writes do not stack identical modal alerts.
  @MainActor private static func watchPreferencesFailures() {
    let shown = PreferencesFailures()
    _ = NotificationCenter.default.addObserver(forName: AppPreferences.refusedWrite, object: nil, queue: .main) { note in
      guard let failure = note.userInfo?[AppPreferences.failureKey] as? AppPreferences.Failure else { return }
      MainActor.assumeIsolated {
        if shown.codes.insert(failure.code).inserted { showPreferencesFailure(failure) }
      }
    }
  }

  @MainActor private final class PreferencesFailures: @unchecked Sendable {
    var codes: Set<String> = []
  }

  @MainActor private static func showPreferencesFailure(_ error: any Error) {
    let alert = NSAlert()
    alert.alertStyle = .warning
    alert.messageText = String(localized: "Something went wrong")
    alert.informativeText = error.localizedDescription
    alert.addButton(withTitle: String(localized: "OK"))
    alert.runModal()
  }

  /// What AppKit reads before the app's code runs, set for a separate profile: it neither restores
  /// nor saves the windows of the person's own copy, and what it draws itself, like the menu bar,
  /// is in the profile's language rather than the one chosen in the person's copy.
  static func isolate(_ profile: Profile) {
    UserDefaults.standard.setVolatileDomain(
      arguments(profile, current: UserDefaults.standard.volatileDomain(forName: UserDefaults.argumentDomain)),
      forName: UserDefaults.argumentDomain)
    forgetWindowPlaces()
  }

  /// AppKit saves where windows and split view dividers are in the person's own settings, which
  /// a profile leaves alone: its windows and split views forget their names for that before
  /// AppKit saves anything under them.
  static func forgetWindowPlaces(_ center: NotificationCenter = .default) {
    // AppKit posts both on the main thread.
    _ = center.addObserver(forName: NSWindow.didUpdateNotification, object: nil, queue: nil) { note in
      let window = note.object as? NSWindow
      MainActor.assumeIsolated {
        if let window, !window.frameAutosaveName.isEmpty { window.setFrameAutosaveName("") }
      }
    }
    _ = center.addObserver(forName: NSSplitView.willResizeSubviewsNotification, object: nil, queue: nil) { note in
      let split = note.object as? NSSplitView
      MainActor.assumeIsolated { split?.autosaveName = nil }
    }
  }

  /// `current` launch arguments with those of `profile` added.
  static func arguments(
    _ profile: Profile, current: [String: Any], system: [String] = AppLanguage.systemLanguages()
  ) -> [String: Any] {
    var arguments = current
    arguments["ApplePersistenceIgnoreState"] = true
    let saved = profile.defaults.persistentDomain(forName: profile.defaultsDomain)?["AppleLanguages"] as? [String]
    arguments["AppleLanguages"] = saved ?? system
    return arguments
  }
}
