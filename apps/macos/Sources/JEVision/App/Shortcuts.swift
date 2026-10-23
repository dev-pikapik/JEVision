import SwiftUI

/// Keyboard shortcuts of the commands that are both in the menus and in the command palette,
/// so the two always show the same keys.
enum Shortcuts {
  static let commandPalette = KeyboardShortcut("k")
  static let newTest = KeyboardShortcut("n")
  static let newProject = KeyboardShortcut("n", modifiers: [.command, .shift])
  static let openProject = KeyboardShortcut("o")
  static let closeProject = KeyboardShortcut("w", modifiers: [.command, .control])
  static let run = KeyboardShortcut("r")
  static let runStepByStep = KeyboardShortcut("r", modifiers: [.command, .option])
  static let runFromSelection = KeyboardShortcut(.return, modifiers: [.command, .shift])
  static let runAll = KeyboardShortcut("r", modifiers: [.command, .control])
  static let record = KeyboardShortcut("r", modifiers: [.command, .shift])
  static let addBlock = KeyboardShortcut("b")
  static let addCondition = KeyboardShortcut("b", modifiers: [.command, .shift])
  static let disconnect = KeyboardShortcut("l", modifiers: [.command, .option])
  static let commit = KeyboardShortcut("c", modifiers: [.command, .option])
  static let newPullRequest = KeyboardShortcut("p", modifiers: [.command, .option])
  static let settings = KeyboardShortcut(",")

  /// How a menu writes `shortcut`, such as "⇧⌘R".
  static func symbols(_ shortcut: KeyboardShortcut) -> String {
    let modifiers: [(EventModifiers, String)] = [(.control, "⌃"), (.option, "⌥"), (.shift, "⇧"), (.command, "⌘")]
    let prefix = modifiers.filter { shortcut.modifiers.contains($0.0) }.map(\.1).joined()
    let key: String =
      switch shortcut.key {
      case .return: "↩"
      case .space: String(localized: "Space")
      case .tab: "⇥"
      case .delete: "⌫"
      case .escape: "⎋"
      default: String(shortcut.key.character).uppercased()
      }
    return prefix + key
  }
}
