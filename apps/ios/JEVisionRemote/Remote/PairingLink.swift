import Foundation

/// A Mac's pairing code as scanned or opened: `jevision://pair?v=1&id=…&name=…&code=…&exp=…`.
/// Only what the screens need is read here; the core checks the rest when pairing.
struct PairingLink: Hashable, Identifiable, Sendable {
  /// The text as scanned, handed to the core unchanged.
  let text: String
  let macName: String
  let expiresAt: Date

  var id: String { text }

  init?(_ text: String) {
    let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard let url = URLComponents(string: text),
      url.scheme?.lowercased() == "jevision",
      url.host?.lowercased() == "pair",
      let items = url.queryItems
    else { return nil }
    func value(_ name: String) -> String? { items.first { $0.name == name }?.value }
    guard value("v") == "1",
      let id = value("id"), id.count == 64, id.allSatisfy(\.isHexDigit),
      let code = value("code"), code.count == 32, code.allSatisfy(\.isHexDigit),
      let name = value("name")?.trimmingCharacters(in: .whitespacesAndNewlines), !name.isEmpty,
      let exp = value("exp").flatMap(TimeInterval.init)
    else { return nil }
    self.text = text
    self.macName = name
    self.expiresAt = Date(timeIntervalSince1970: exp)
  }

  init?(_ url: URL) {
    self.init(url.absoluteString)
  }

  /// Whether the code has run out by this iPhone's clock. The Mac's clock decides in the end,
  /// so this only saves a pointless attempt.
  func isExpired(at now: Date) -> Bool {
    now >= expiresAt
  }
}
