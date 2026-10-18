import CoreGraphics

/// A key combination as test files write it (`cmd+shift+s`), as virtual key codes of the
/// US layout. macOS matches shortcuts to the same keys on other layouts.
public struct KeyStroke: Equatable, Sendable {
  public static let escapeKeyCode: Int64 = 53

  public let key: CGKeyCode
  /// Modifier keys in the order to press them.
  public let modifiers: [CGKeyCode]
  public let flags: CGEventFlags

  private static let modifierKeys: [String: CGKeyCode] = ["cmd": 55, "shift": 56, "option": 58, "ctrl": 59]
  static let keys: [String: CGKeyCode] = [
    "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9, "b": 11,
    "q": 12, "w": 13, "e": 14, "r": 15, "y": 16, "t": 17, "1": 18, "2": 19, "3": 20, "4": 21,
    "6": 22, "5": 23, "=": 24, "9": 25, "7": 26, "-": 27, "8": 28, "0": 29, "]": 30, "o": 31,
    "u": 32, "[": 33, "i": 34, "p": 35, "l": 37, "j": 38, "'": 39, "k": 40, ";": 41, "\\": 42,
    ",": 43, "/": 44, "n": 45, "m": 46, ".": 47, "`": 50,
    "return": 36, "tab": 48, "space": 49, "delete": 51, "escape": 53, "forwarddelete": 117,
    "home": 115, "end": 119, "pageup": 116, "pagedown": 121,
    "left": 123, "right": 124, "down": 125, "up": 126,
    "f1": 122, "f2": 120, "f3": 99, "f4": 118, "f5": 96, "f6": 97, "f7": 98, "f8": 100,
    "f9": 101, "f10": 109, "f11": 103, "f12": 111, "f13": 105, "f14": 107, "f15": 113,
    "f16": 106, "f17": 64, "f18": 79, "f19": 80, "f20": 90,
  ]

  /// `nil` for text that isn't a key combination.
  public init?(_ text: String) {
    let parts = text.split(separator: "+", omittingEmptySubsequences: false).map(String.init)
    guard let last = parts.last, let key = Self.keys[last] else { return nil }
    var modifiers: [CGKeyCode] = []
    var flags: CGEventFlags = []
    for name in parts.dropLast() {
      guard let code = Self.modifierKeys[name], !modifiers.contains(code) else { return nil }
      modifiers.append(code)
      flags.insert(Self.flag(of: code))
    }
    self.key = key
    self.modifiers = modifiers
    self.flags = flags
  }

  /// The flag a modifier key sets while held.
  public static func flag(of modifier: CGKeyCode) -> CGEventFlags {
    switch modifier {
    case 55: .maskCommand
    case 56: .maskShift
    case 58: .maskAlternate
    default: .maskControl
    }
  }
}
