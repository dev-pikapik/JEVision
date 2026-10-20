import AppKit
import Foundation
import JEVisionHands

/// Key combinations as the file writes them (`cmd+shift+s`) and as people read them (⇧⌘S).
enum KeyCombo {
  /// Modifiers in the order the file writes them.
  private static let modifiers: [(name: String, flag: NSEvent.ModifierFlags)] = [
    ("cmd", .command), ("shift", .shift), ("option", .option), ("ctrl", .control),
  ]
  /// Modifiers in the order macOS menus show them.
  private static let symbols: [(name: String, symbol: String)] = [
    ("ctrl", "⌃"), ("option", "⌥"), ("shift", "⇧"), ("cmd", "⌘"),
  ]
  private static let keySymbols: [String: String] = [
    "return": "↩", "tab": "⇥", "delete": "⌫", "forwarddelete": "⌦", "escape": "⎋",
    "home": "↖", "end": "↘", "pageup": "⇞", "pagedown": "⇟",
    "up": "↑", "down": "↓", "left": "←", "right": "→",
  ]

  /// `cmd+shift+s` → `⇧⌘S`, with the letter the keyboard layout prints on that key: `⇧⌘Ы` on a
  /// Russian one. Without a layout, the key's name on a US keyboard.
  static func display(_ keys: String, layout: Data? = ActiveKeyboard.shared.layout) -> String {
    let parts = keys.split(separator: "+", omittingEmptySubsequences: false).map(String.init)
    guard let key = parts.last else { return keys }
    let held = symbols.filter { parts.dropLast().contains($0.name) }.map(\.symbol).joined()
    let name = key == "space" ? String(localized: "Space") : keySymbols[key] ?? printed(key, on: layout) ?? key.uppercased()
    return held + name
  }

  /// What a letter, digit or punctuation key shows on the layout; `nil` for any other key.
  private static func printed(_ key: String, on layout: Data?) -> String? {
    guard let layout, key.count == 1, let code = KeyStroke(key)?.key,
      let text = KeyboardLayout.character(keyCode: code, layout: layout)
    else { return nil }
    let upper = text.uppercased()
    return upper.count == text.count ? upper : text
  }

  /// What the file writes for a key press, by the key's place on the keyboard rather than its
  /// letter, so it is the same on every layout. `nil` for keys a test can't press.
  static func from(keyCode: UInt16, modifierFlags: NSEvent.ModifierFlags) -> String? {
    guard let key = KeyStroke.names[keyCode] else { return nil }
    let held = modifiers.filter { modifierFlags.contains($0.flag) }.map(\.name)
    return (held + [key]).joined(separator: "+")
  }
}
