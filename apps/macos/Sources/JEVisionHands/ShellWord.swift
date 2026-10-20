import Foundation

/// Text as one word of a shell command, quoted so the shell passes it on unchanged.
public func shellWord(_ text: String) -> String {
  let plain = CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-/:@%+=,")
  if !text.isEmpty, text.unicodeScalars.allSatisfy(plain.contains) { return text }
  return "'" + text.replacing("'", with: "'\\''") + "'"
}
