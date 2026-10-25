import Foundation
import JEVisionCore

/// Whether runs record a video of the app under test and how long videos stay, as chosen in
/// Settings → Advanced. The app and the `jevision` tool read the same choice.
public struct VideoSettings {
  public static let modeKey = "video.mode"
  public static let keepDaysKey = "video.keepDays"
  public static let defaultMode = VideoMode.onFailure
  public static let defaultKeepDays = 30
  /// How long videos can stay, in days; 0 keeps them all.
  public static let keepChoices = [7, 30, 90, 0]

  /// The app's settings, from the app or from the `jevision` tool.
  public static var app: VideoSettings { VideoSettings(Profile.current.appDefaults) }

  let defaults: UserDefaults

  public init(_ defaults: UserDefaults) {
    self.defaults = defaults
  }

  public var mode: VideoMode {
    defaults.string(forKey: Self.modeKey).flatMap(VideoMode.init(name:)) ?? Self.defaultMode
  }

  /// Days a video stays; `nil` keeps them all.
  public var keepDays: UInt32? {
    let days = defaults.object(forKey: Self.keepDaysKey) as? Int ?? Self.defaultKeepDays
    guard days != 0 else { return nil }
    return UInt32(exactly: days).flatMap { $0 > 0 ? $0 : nil } ?? UInt32(Self.defaultKeepDays)
  }
}

extension VideoMode {
  /// The choices in the order they are offered.
  public static let all: [VideoMode] = [.off, .onFailure, .always]

  /// The name in the settings and on the command line.
  public var name: String {
    switch self {
    case .off: "off"
    case .onFailure: "on-failure"
    case .always: "always"
    }
  }

  public init?(name: String) {
    guard let mode = Self.all.first(where: { $0.name == name }) else { return nil }
    self = mode
  }
}
