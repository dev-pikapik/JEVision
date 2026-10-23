import Foundation

/// Text output. Colours only on a terminal, and not when `NO_COLOR` is set.
struct Console: Sendable {
  let write: @Sendable (String) -> Void
  /// Escape codes for colours are welcome.
  let color: Bool
  /// A line can be rewritten in place, to show what is going on until it's done.
  let live: Bool

  static let standardOutput = Console(descriptor: STDOUT_FILENO, handle: .standardOutput)
  static let standardError = Console(descriptor: STDERR_FILENO, handle: .standardError)

  init(write: @escaping @Sendable (String) -> Void, color: Bool = false, live: Bool = false) {
    self.write = write
    self.color = color
    self.live = live
  }

  private init(descriptor: Int32, handle: FileHandle) {
    let environment = ProcessInfo.processInfo.environment
    let terminal = isatty(descriptor) == 1 && environment["TERM"] != "dumb"
    self.init(
      write: { handle.write(Data($0.utf8)) },
      color: terminal && (environment["NO_COLOR"] ?? "").isEmpty,
      live: terminal)
  }

  func line(_ text: String = "") {
    write(text + "\n")
  }

  func paint(_ text: String, _ style: Style) -> String {
    color ? "\u{1B}[\(style.rawValue)m\(text)\u{1B}[0m" : text
  }

  enum Style: String {
    case bold = "1"
    case dim = "2"
    case red = "31"
    case green = "32"
    case yellow = "33"
  }
}

/// A JSON value that keeps the order of object members, printed like serde_json's pretty printer.
indirect enum JSON {
  case null
  case bool(Bool)
  case int(Int64)
  case string(String)
  case array([JSON])
  case object([(String, JSON)])

  static func string(_ value: String?) -> JSON {
    value.map { .string($0) } ?? .null
  }

  /// What a command prints with `--json`: an object whose first member, `jsonVersion`, tells the
  /// layout, so a script can tell a layout it doesn't know from one it reads.
  static func result(_ members: [(String, JSON)]) -> JSON {
    .object([("jsonVersion", .int(1))] + members)
  }

  var pretty: String {
    var out = ""
    write(to: &out, indent: "")
    return out
  }

  private func write(to out: inout String, indent: String) {
    switch self {
    case .null: out += "null"
    case .bool(let value): out += value ? "true" : "false"
    case .int(let value): out += String(value)
    case .string(let value): out += Self.quoted(value)
    case .array(let items):
      guard !items.isEmpty else { return out += "[]" }
      out += "[\n"
      for (index, item) in items.enumerated() {
        out += indent + "  "
        item.write(to: &out, indent: indent + "  ")
        out += index < items.count - 1 ? ",\n" : "\n"
      }
      out += indent + "]"
    case .object(let members):
      guard !members.isEmpty else { return out += "{}" }
      out += "{\n"
      for (index, (key, value)) in members.enumerated() {
        out += indent + "  " + Self.quoted(key) + ": "
        value.write(to: &out, indent: indent + "  ")
        out += index < members.count - 1 ? ",\n" : "\n"
      }
      out += indent + "}"
    }
  }

  private static func quoted(_ text: String) -> String {
    var out = "\""
    for scalar in text.unicodeScalars {
      switch scalar {
      case "\"": out += "\\\""
      case "\\": out += "\\\\"
      case "\n": out += "\\n"
      case "\r": out += "\\r"
      case "\t": out += "\\t"
      case "\u{08}": out += "\\b"
      case "\u{0C}": out += "\\f"
      case _ where scalar.value < 0x20: out += String(format: "\\u%04x", scalar.value)
      default: out.unicodeScalars.append(scalar)
      }
    }
    return out + "\""
  }
}
