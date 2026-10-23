import Foundation

/// What a run does with the person's own mouse and keyboard, as chosen in Settings → General →
/// While a test runs. The watcher asks at the moment it matters, so a change counts at once, in
/// a run that is already going. The app and the `jevision` tool read the same choice.
public struct RunInputSettings: Equatable, Sendable {
  public static let pausesOnMouseKey = "run.pausesOnMouse"
  public static let escapeStopsKey = "run.escapeStops"
  public static let collapsesKey = "run.collapsesToPanel"

  /// The run pauses when the person touches the mouse or keyboard.
  public var pausesOnMouse = true
  /// Esc pressed twice, in any app, stops the run.
  public var escapeStops = true
  /// JEVision steps aside to the small panel while a test runs.
  public var collapsesToPanel = true

  public init() {}

  /// The app's settings, from the app or from the `jevision` tool.
  public static var app: RunInputSettings { RunInputSettings(Profile.current.appDefaults) }

  public init(_ defaults: UserDefaults) {
    pausesOnMouse = defaults.object(forKey: Self.pausesOnMouseKey) as? Bool ?? true
    escapeStops = defaults.object(forKey: Self.escapeStopsKey) as? Bool ?? true
    collapsesToPanel = defaults.object(forKey: Self.collapsesKey) as? Bool ?? true
  }
}
