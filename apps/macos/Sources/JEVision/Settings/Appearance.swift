import AppKit
import JEVisionHands

enum Appearance: String, CaseIterable, Identifiable {
  case system, light, dark

  static let key = "appearance"

  static var saved: Appearance {
    Appearance(rawValue: Profile.current.defaults.string(forKey: key) ?? "") ?? .system
  }

  var id: Self { self }

  var title: String {
    switch self {
    case .system: String(localized: "System")
    case .light: String(localized: "Light")
    case .dark: String(localized: "Dark")
    }
  }

  @MainActor
  func apply() {
    NSApplication.shared.appearance = switch self {
    case .system: nil
    case .light: NSAppearance(named: .aqua)
    case .dark: NSAppearance(named: .darkAqua)
    }
  }
}
