import Foundation
import JEVisionCore

/// The language of what `jevision` prints. Every text is a key of the shared catalog; the core puts
/// the values in and words it in the language chosen here, as it does for the app.
enum CLILanguage {
  /// `--lang <code>` or `--lang=<code>`, allowed anywhere before `--`: its value, and the arguments
  /// without it. The code has to be a language of the catalog.
  static func extract(_ arguments: [String], available: [String] = catalogLanguages()) throws(UsageError) -> (code: String?, rest: [String]) {
    var rest: [String] = []
    var code: String?
    var queue = arguments[...]
    while let argument = queue.popFirst() {
      if argument == "--" {
        rest.append(argument)
        rest.append(contentsOf: queue)
        break
      }
      guard argument == "--lang" || argument.hasPrefix("--lang=") else {
        rest.append(argument)
        continue
      }
      guard code == nil else { throw UsageError(message: words("cli.error.optionTwice", "--lang")) }
      let value = argument.hasPrefix("--lang=") ? String(argument.dropFirst("--lang=".count)) : queue.popFirst() ?? ""
      guard !value.isEmpty else { throw UsageError(message: words("cli.error.optionNeedsValue", "--lang")) }
      let base = value.split(whereSeparator: { "-_.@".contains($0) }).first.map { $0.lowercased() } ?? ""
      guard available.contains(base) else {
        throw UsageError(message: words("cli.error.noLanguage", value, available.joined(separator: ", ")))
      }
      code = base
    }
    return (code, rest)
  }

  /// The language when `--lang` isn't given: English on a CI machine, where logs are read by
  /// everyone on the team; otherwise the first of the Mac's languages that the catalog has.
  static func resolve(environment: [String: String], preferred: [String], available: [String] = catalogLanguages()) -> String {
    if environment["CI"].map({ !$0.isEmpty }) == true { return "en" }
    return Bundle.preferredLocalizations(from: available, forPreferences: preferred).first ?? "en"
  }
}

/// The text of the catalog key in the language of the command line. Whole numbers count for
/// plurals; anything else is written as it is.
func words(_ key: String, _ values: Any...) -> String {
  localizedText(
    key: key,
    args: values.map { value in
      switch value {
      case let number as any BinaryInteger: .number(value: Int64(clamping: number))
      default: .text(value: String(describing: value))
      }
    })
}
