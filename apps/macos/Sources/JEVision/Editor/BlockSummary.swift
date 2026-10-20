import Foundation
import JEVisionCore
import JEVisionHands

/// The one line under a block's title that says what exactly it does.
enum BlockSummary {
  /// A `${data.name}` in it stays whole when the line wraps. `testApp` is the test's own app,
  /// which an Open App block without one launches.
  static func text(_ kind: BlockKind, appName: (String) -> String, testApp: String? = nil) -> String {
    unbreakablePlaceholders(text: plainText(kind, appName: appName, testApp: testApp))
  }

  private static func plainText(_ kind: BlockKind, appName: (String) -> String, testApp: String?) -> String {
    switch kind {
    case .openApp(let app, _, _, _, _):
      return (app ?? testApp).map(appName) ?? String(localized: "The app under test")
    case .openFile(let files, _, _, _, _):
      guard let first = files.first else { return String(localized: "No file chosen") }
      let name = (first as NSString).lastPathComponent
      return files.count == 1 ? name : String(localized: "\(name) and \(files.count - 1) more")
    case .click(let target, let button, let double, let modifiers, _, _):
      guard isUnset(target) else {
        let place = targetName(target)
        switch (button, double) {
        case (.right, _): return holding(modifiers, String(localized: "Right-click \(place)"))
        case (.middle, _): return holding(modifiers, String(localized: "Middle-click \(place)"))
        case (.left, true): return holding(modifiers, String(localized: "Double-click \(place)"))
        case (.left, false): return holding(modifiers, String(localized: "Click \(place)"))
        }
      }
      return String(localized: "Choose what to click")
    case .hover(let target, let modifiers, _, _):
      guard !isUnset(target) else { return String(localized: "Choose what to point at") }
      return holding(modifiers, String(localized: "Point at \(targetName(target))"))
    case .drag(let from, let to, let button, let modifiers, _, _):
      guard !isUnset(from), !isUnset(to) else { return String(localized: "Choose what to drag and where") }
      let text =
        switch button {
        case .left: String(localized: "Drag \(spotName(from)) to \(spotName(to))")
        case .right: String(localized: "Right-drag \(spotName(from)) to \(spotName(to))")
        case .middle: String(localized: "Middle-drag \(spotName(from)) to \(spotName(to))")
        }
      return holding(modifiers, text)
    case .scroll(let direction, let amount, let unit, let at, let modifiers, _, _):
      let much = unit == .lines ? String(localized: "\(Int(amount)) lines") : String(localized: "\(Int(amount)) px")
      guard let at, !isUnset(at) else {
        switch direction {
        case .up: return holding(modifiers, String(localized: "Scroll up \(much)"))
        case .down: return holding(modifiers, String(localized: "Scroll down \(much)"))
        case .left: return holding(modifiers, String(localized: "Scroll left \(much)"))
        case .right: return holding(modifiers, String(localized: "Scroll right \(much)"))
        }
      }
      let place = targetName(at)
      switch direction {
      case .up: return holding(modifiers, String(localized: "Scroll up \(much) over \(place)"))
      case .down: return holding(modifiers, String(localized: "Scroll down \(much) over \(place)"))
      case .left: return holding(modifiers, String(localized: "Scroll left \(much) over \(place)"))
      case .right: return holding(modifiers, String(localized: "Scroll right \(much) over \(place)"))
      }
    case .type(let text, let into, _):
      guard !text.isEmpty else { return String(localized: "No text yet") }
      let what = secretName(text).map { String(localized: "secret \($0)") } ?? quoted(text)
      // A point says nothing about the field, so only a picture, a description or an area names it.
      if let into, !isUnset(into) {
        switch into {
        case .point: break
        default: return String(localized: "Type \(what) into \(targetName(into))")
        }
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
    case .runTest(let test, _, let inputs, _):
      if test.isEmpty { return String(localized: "Choose a test") }
      guard isBlockPath(path: test), !inputs.isEmpty else {
        return String(localized: "Runs \(quoted(ProjectScanner.testName(path: test)))")
      }
      return inputs.sorted { $0.key < $1.key }
        .map { "\($0.key): \(secretName($0.value).map { String(localized: "secret \($0)") } ?? quoted($0.value, limit: 24))" }
        .joined(separator: " · ")
    case .clipboard(let action, _):
      switch action {
      case .put(let text):
        return text.isEmpty ? String(localized: "No text yet") : String(localized: "Put \(quoted(text, limit: 32)) on the clipboard")
      case .putImage(let image):
        return image.isEmpty ? String(localized: "No picture yet") : String(localized: "Put \(imageName(image)) on the clipboard")
      case .read(let saveAs, let content):
        guard !saveAs.isEmpty else { return String(localized: "Choose a name to save it as") }
        switch content {
        case .text: return String(localized: "Read the clipboard → \(saveAs)")
        case .image: return String(localized: "Read the picture on the clipboard → \(saveAs)")
        case .files: return String(localized: "Read the copied files → \(saveAs)")
        }
      case .check(let test, let to, let timeoutMs):
        return String(localized: "Until the clipboard \(sign(of: test)) \(quoted(to)) · \(duration(timeoutMs))")
      }
    case .dateTime(let saveAs, let format, let language, let timeZone, let shift, _):
      guard !saveAs.isEmpty else { return String(localized: "Choose a name to save it as") }
      let names = language.map { " · " + AppLanguage.name($0) } ?? ""
      let zone = timeZone.map { " · " + $0 } ?? ""
      let moved = shift.map { " · " + BlockInfo.timeUnitName($0.unit).lowercased() + " " + signed($0.amount) } ?? ""
      return quoted(format, limit: 24) + names + zone + moved + " → " + saveAs
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

  /// What a click, drag, hover or scroll does, with the keys held down during it.
  private static func holding(_ modifiers: Modifiers, _ text: String) -> String {
    modifiers.isEmpty ? text : String(localized: "\(text) holding \(modifiers.symbols)")
  }

  private static func signed(_ amount: Int32) -> String { amount > 0 ? "+\(amount)" : "\(amount)" }

  /// `${var.total} > 3`, with the symbols of the comparison so it reads the same in every language.
  static func valueCheck(_ check: ValueCheck) -> String {
    "\(check.of) \(sign(of: check.is)) \(quoted(check.to))"
  }

  private static func sign(of test: ValueTest) -> String {
    switch test {
    case .equals: "="
    case .notEquals: "≠"
    case .contains: "∋"
    case .greaterThan: ">"
    case .lessThan: "<"
    }
  }

  /// `refs/sign-in-button.png` → “sign in button”; a picture a block saves, like
  /// `${var.shot}`, as it is written.
  static func imageName(_ path: String) -> String {
    if path.contains("${") { return quoted(path, limit: 32) }
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
    let language = AppLanguage.shared.locale
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
    case .area(let area): String(localized: "area \(quoted(area, limit: 32))")
    }
  }

  /// A target without the “at” in front of a point.
  private static func spotName(_ target: Target) -> String {
    switch target {
    case .image(let path): imageName(path)
    case .point(let p): "\(percent(p.x)), \(percent(p.y))"
    case .description(let text): quoted(text, limit: 40)
    case .area(let area): quoted(area, limit: 32)
    }
  }

  /// A picture not chosen, an element not described or an area not given yet.
  private static func isUnset(_ target: Target) -> Bool {
    switch target {
    case .image(let path): path.isEmpty
    case .description(let text), .area(let text): text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
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
