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
    guard let items = Self.query(text) else { return nil }
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

  /// Whether `text` is a pairing code from a newer JEVision: its format `v` is a number above
  /// the one this version reads. Pairing with it needs JEVision Remote updated first.
  static func isFromNewerVersion(_ text: String) -> Bool {
    let version = query(text.trimmingCharacters(in: .whitespacesAndNewlines))?.first { $0.name == "v" }?.value
    return version.flatMap { UInt($0) }.map { $0 > 1 } ?? false
  }

  /// The query of a `jevision://pair?…` text; `nil` for any other text.
  private static func query(_ text: String) -> [URLQueryItem]? {
    guard let url = URLComponents(string: text),
      url.scheme?.lowercased() == "jevision",
      url.host?.lowercased() == "pair"
    else { return nil }
    return url.queryItems
  }

  /// Whether the code has run out by this iPhone's clock. The Mac's clock decides in the end,
  /// so this only saves a pointless attempt.
  func isExpired(at now: Date) -> Bool {
    now >= expiresAt
  }
}
