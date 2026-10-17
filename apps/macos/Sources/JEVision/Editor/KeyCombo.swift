import AppKit

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
  private static let namedKeys: [UInt16: String] = [
    36: "return", 48: "tab", 49: "space", 51: "delete", 53: "escape", 117: "forwarddelete",
    115: "home", 119: "end", 116: "pageup", 121: "pagedown",
    123: "left", 124: "right", 125: "down", 126: "up",
    122: "f1", 120: "f2", 99: "f3", 118: "f4", 96: "f5", 97: "f6", 98: "f7", 100: "f8", 101: "f9",
    109: "f10", 103: "f11", 111: "f12", 105: "f13", 107: "f14", 113: "f15", 106: "f16",
    64: "f17", 79: "f18", 80: "f19", 90: "f20",
  ]
  private static let keySymbols: [String: String] = [
    "return": "↩", "tab": "⇥", "delete": "⌫", "forwarddelete": "⌦", "escape": "⎋",
    "home": "↖", "end": "↘", "pageup": "⇞", "pagedown": "⇟",
    "up": "↑", "down": "↓", "left": "←", "right": "→",
  ]
  private static let punctuation = Set("-=[]\\;',./`")

  /// `cmd+shift+s` → `⇧⌘S`.
  static func display(_ keys: String) -> String {
    let parts = keys.split(separator: "+", omittingEmptySubsequences: false).map(String.init)
    guard let key = parts.last else { return keys }
    let held = symbols.filter { parts.dropLast().contains($0.name) }.map(\.symbol).joined()
    let name = key == "space" ? String(localized: "Space") : keySymbols[key] ?? key.uppercased()
    return held + name
  }

  /// What the file writes for a key press: `characters` is the key's text without modifiers.
  /// `nil` for keys a test can't press.
  static func from(keyCode: UInt16, characters: String?, modifierFlags: NSEvent.ModifierFlags) -> String? {
    let key: String
    if let named = namedKeys[keyCode] {
      key = named
    } else if let c = characters?.lowercased(), c.count == 1, let ch = c.first,
      ch.isASCII && (ch.isLetter || ch.isNumber) || punctuation.contains(ch)
    {
      key = c
    } else {
      return nil
    }
    let held = modifiers.filter { modifierFlags.contains($0.flag) }.map(\.name)
    return (held + [key]).joined(separator: "+")
  }
}
