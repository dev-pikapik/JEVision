import Foundation
import JEVisionCore

/// The one line under a block's title that says what exactly it does.
enum BlockSummary {
  /// A `${data.name}` in it stays whole when the line wraps. `testApp` is the test's own app,
  /// which an Open App block without one launches.
  static func text(_ kind: BlockKind, appName: (String) -> String, testApp: String? = nil) -> String {
    unbreakablePlaceholders(text: plainText(kind, appName: appName, testApp: testApp))
  }

  private static func plainText(_ kind: BlockKind, appName: (String) -> String, testApp: String?) -> String {
    switch kind {
    case .openApp(let app, _):
      return (app ?? testApp).map(appName) ?? String(localized: "The app under test")
    case .openFile(let files, _, _, _, _):
      guard let first = files.first else { return String(localized: "No file chosen") }
      let name = (first as NSString).lastPathComponent
      return files.count == 1 ? name : String(localized: "\(name) and \(files.count - 1) more")
    case .click(let target, let button, let double, _, _):
      guard isUnset(target) else {
        let place = targetName(target)
        switch (button, double) {
        case (.right, _): return String(localized: "Right-click \(place)")
        case (.left, true): return String(localized: "Double-click \(place)")
        case (.left, false): return String(localized: "Click \(place)")
        }
      }
      return String(localized: "Choose what to click")
    case .hover(let target, _, _):
      return isUnset(target) ? String(localized: "Choose what to point at") : String(localized: "Point at \(targetName(target))")
    case .drag(let from, let to, _, _):
      return isUnset(from) || isUnset(to)
        ? String(localized: "Choose what to drag and where")
        : String(localized: "Drag \(spotName(from)) to \(spotName(to))")
    case .scroll(let direction, let amount, let unit, let at, _, _):
      let much = unit == .lines ? String(localized: "\(Int(amount)) lines") : String(localized: "\(Int(amount)) px")
      guard let at, !isUnset(at) else {
        switch direction {
        case .up: return String(localized: "Scroll up \(much)")
        case .down: return String(localized: "Scroll down \(much)")
        case .left: return String(localized: "Scroll left \(much)")
        case .right: return String(localized: "Scroll right \(much)")
        }
      }
      let place = targetName(at)
      switch direction {
      case .up: return String(localized: "Scroll up \(much) over \(place)")
      case .down: return String(localized: "Scroll down \(much) over \(place)")
      case .left: return String(localized: "Scroll left \(much) over \(place)")
      case .right: return String(localized: "Scroll right \(much) over \(place)")
      }
    case .type(let text, let into, _):
      guard !text.isEmpty else { return String(localized: "No text yet") }
      let what = secretName(text).map { String(localized: "secret \($0)") } ?? quoted(text)
      if let into, case .image(let path) = into, !path.isEmpty {
        return String(localized: "Type \(what) into \(imageName(path))")
      }
      if let into, case .description = into, !isUnset(into) {
        return String(localized: "Type \(what) into \(targetName(into))")
      }
      return String(localized: "Type \(what)")
    case .pressKeys(let keys, _):
      return keys.isEmpty ? String(localized: "No keys recorded") : String(localized: "Press \(KeyCombo.display(keys))")
    case .waitFor(let until, let timeoutMs, _):
      let limit = duration(timeoutMs)
      switch until {
      case .appears(let reference):
        return String(localized: "Until \(referenceName(reference)) appears · \(limit)")
      case .disappears(let reference):
        return String(localized: "Until \(referenceName(reference)) disappears · \(limit)")
      case .screenStable:
        return String(localized: "Until the screen stops changing · \(limit)")
      }
    case .pause(let durationMs, _):
      return duration(durationMs)
    case .check(let reference, _):
      if let expect = expectation(reference) {
        return judged(String(localized: "Screen shows \(quoted(expect, limit: 40))"), by: reference)
      }
      return reference.image.isEmpty
        ? String(localized: "Choose how the screen should look")
        : judged(String(localized: "Screen matches \(imageName(reference.image))"), by: reference)
    case .if(let condition, _, let elseIf, _):
      let first: String
      switch condition {
      case .screen(let reference):
        let text =
          if let expect = expectation(reference) {
            String(localized: "If the screen shows \(quoted(expect, limit: 40))")
          } else if reference.image.isEmpty {
            String(localized: "Choose what to look for")
          } else {
            String(localized: "If \(imageName(reference.image)) is on screen")
          }
        first = judged(text, by: reference)
      case .value(let check):
        first =
          check.of.isEmpty
          ? String(localized: "Choose a value to check")
          : String(localized: "If \(valueCheck(check))")
      }
      return elseIf.isEmpty ? first : first + " · " + String(localized: "or \(elseIf.count) more")
    case .repeat(let times, _, _):
      return String(localized: "\(Int(times)) times")
    case .useData(let file, _, _):
      return file.isEmpty
        ? String(localized: "Choose a CSV file")
        : String(localized: "For each row of \((file as NSString).lastPathComponent)")
    case .runTest(let test, let inputs, _):
      if test.isEmpty { return String(localized: "Choose a test") }
      let file = (test as NSString).lastPathComponent
      guard file.lowercased().hasSuffix(ProjectLibrary.blockSuffix) else {
        return String(localized: "Runs \(quoted(ProjectScanner.testName(file)))")
      }
      guard !inputs.isEmpty else {
        return String(localized: "Runs \(quoted(ProjectScanner.testName(file)))")
      }
      return inputs.sorted { $0.key < $1.key }
        .map { "\($0.key): \(secretName($0.value).map { String(localized: "secret \($0)") } ?? quoted($0.value, limit: 24))" }
        .joined(separator: " · ")
    case .script(let language, let code, _, let outputs, _, _):
      let line = code.split(whereSeparator: \.isNewline).first { !$0.allSatisfy(\.isWhitespace) }
      guard let line else { return String(localized: "No code yet") }
      let text = BlockInfo.languageName(language) + ": " + quoted(line.trimmingCharacters(in: .whitespaces), limit: 32)
      return outputs.isEmpty ? text : text + " → " + outputs.map(\.name).joined(separator: ", ")
    case .command(let command, _, _, let saveOutput, _, _):
      guard !command.isEmpty else { return String(localized: "No command yet") }
      let text = quoted(command, limit: 32)
      return saveOutput.map { text + " → " + $0 } ?? text
    case .http(let method, let url, _, _, _, let save, _, _):
      guard !url.isEmpty else { return String(localized: "No address yet") }
      let text = BlockInfo.methodName(method) + " " + url
      return save.isEmpty ? text : text + " → " + save.map(\.name).joined(separator: ", ")
    case .checkFile(let path, let until, let timeoutMs, _):
      guard !path.isEmpty else { return String(localized: "Choose a file") }
      let name = quoted((path as NSString).lastPathComponent)
      let limit = duration(timeoutMs)
      switch until {
      case .exists: return String(localized: "Until \(name) appears · \(limit)")
      case .missing: return String(localized: "Until \(name) disappears · \(limit)")
      case .changed: return String(localized: "Until \(name) changes · \(limit)")
      case .contains(let text): return String(localized: "Until \(name) contains \(quoted(text)) · \(limit)")
      }
    case .database(let file, let query, let saveAs, _, _, _):
      guard !file.isEmpty else { return String(localized: "Choose a database") }
      let text = quoted((file as NSString).lastPathComponent) + ": " + quoted(query, limit: 32)
      return saveAs.map { text + " → " + $0 } ?? text
    }
  }

  /// `${var.total} > 3`, with the symbols of the comparison so it reads the same in every language.
  static func valueCheck(_ check: ValueCheck) -> String {
    let sign =
      switch check.is {
      case .equals: "="
      case .notEquals: "≠"
      case .contains: "∋"
      case .greaterThan: ">"
      case .lessThan: "<"
      }
    return "\(check.of) \(sign) \(quoted(check.to))"
  }

  /// `refs/sign-in-button.png` → “sign in button”.
  static func imageName(_ path: String) -> String {
    let base = ((path as NSString).lastPathComponent as NSString).deletingPathExtension
    return quoted(base.replacing(/[-_]+/, with: " ").trimmingCharacters(in: .whitespaces))
  }

  /// On one line: Return and Tab show as the symbols on their keys. The quotation marks are those of
  /// the language the app speaks: “…” in English, «…» in Russian. Text too long to show is cut
  /// before a `${…}` placeholder rather than inside it.
  static func quoted(_ text: String, limit: Int = 24) -> String {
    let line = String(text.map { $0.isNewline ? "⏎" : $0 == "\t" ? "⇥" : $0 })
    var short = line
    if line.count > limit {
      var head = line.prefix(limit - 1)
      if let open = head.range(of: "${", options: .backwards), !head[open.upperBound...].contains("}") {
        head = head[..<open.lowerBound]
      }
      short = head.trimmingCharacters(in: .whitespaces) + "…"
    }
    let language = Locale(identifier: Bundle.main.preferredLocalizations.first ?? "en")
    return (language.quotationBeginDelimiter ?? "“") + short + (language.quotationEndDelimiter ?? "”")
  }

  /// The secret's name when the whole text is one `${secret.NAME}` placeholder.
  static func secretName(_ text: String) -> String? {
    text.wholeMatch(of: /\$\{secret\.([A-Za-z0-9_]+)\}/).map { String($0.1) }
  }

  static func duration(_ ms: UInt64) -> String {
    Duration.milliseconds(Int64(clamping: ms))
      .formatted(.units(allowed: [.hours, .minutes, .seconds, .milliseconds], width: .abbreviated))
  }

  private static func targetName(_ target: Target) -> String {
    switch target {
    case .image(let path): imageName(path)
    case .point(let p): String(localized: "at \(percent(p.x)), \(percent(p.y))")
    case .description(let text): quoted(text, limit: 40)
    }
  }

  /// A target without the “at” in front of a point.
  private static func spotName(_ target: Target) -> String {
    switch target {
    case .image(let path): imageName(path)
    case .point(let p): "\(percent(p.x)), \(percent(p.y))"
    case .description(let text): quoted(text, limit: 40)
    }
  }

  /// A picture not chosen or an element not described yet.
  private static func isUnset(_ target: Target) -> Bool {
    switch target {
    case .image(let path): path.isEmpty
    case .description(let text): text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    case .point: false
    }
  }

  private static func referenceName(_ reference: Reference) -> String {
    if let expect = expectation(reference) { return quoted(expect, limit: 40) }
    return reference.image.isEmpty ? String(localized: "an image") : imageName(reference.image)
  }

  /// What a comparison by meaning expects, when it says.
  private static func expectation(_ reference: Reference) -> String? {
    guard case .meaning(let expect, _, _) = reference.compare else { return nil }
    let text = expect.trimmingCharacters(in: .whitespacesAndNewlines)
    return text.isEmpty ? nil : text
  }

  /// `text`, followed by the AI the block asks when it names one instead of the settings' one.
  private static func judged(_ text: String, by reference: Reference) -> String {
    guard case .meaning(_, _, let id) = reference.compare, !id.isEmpty else { return text }
    return text + " · " + (visionProviders().first { $0.id == id }?.name ?? id)
  }

  static func percent(_ fraction: Double) -> String {
    fraction.formatted(.percent.precision(.fractionLength(0...1)))
  }
}
