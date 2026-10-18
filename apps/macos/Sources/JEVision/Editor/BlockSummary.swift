import Foundation
import JEVisionCore

/// The one line under a block's title that says what exactly it does.
enum BlockSummary {
  /// `testApp` is the test's own app, which an Open App block without one launches.
  static func text(_ kind: BlockKind, appName: (String) -> String, testApp: String? = nil) -> String {
    switch kind {
    case .openApp(let app, _):
      return (app ?? testApp).map(appName) ?? String(localized: "The app under test")
    case .openFile(let files, _, _, _):
      guard let first = files.first else { return String(localized: "No file chosen") }
      let name = (first as NSString).lastPathComponent
      return files.count == 1 ? name : String(localized: "\(name) and \(files.count - 1) more")
    case .click(let target, let button, let double, _, _):
      guard case .image(let path) = target, path.isEmpty else {
        let place = targetName(target)
        switch (button, double) {
        case (.right, _): return String(localized: "Right-click \(place)")
        case (.left, true): return String(localized: "Double-click \(place)")
        case (.left, false): return String(localized: "Click \(place)")
        }
      }
      return String(localized: "Choose what to click")
    case .type(let text, let into, _):
      guard !text.isEmpty else { return String(localized: "No text yet") }
      let what = secretName(text).map { String(localized: "secret \($0)") } ?? quoted(text)
      if let into, case .image(let path) = into, !path.isEmpty {
        return String(localized: "Type \(what) into \(imageName(path))")
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
    case .check(let reference, _):
      if let expect = expectation(reference) { return String(localized: "Screen shows \(quoted(expect, limit: 40))") }
      return reference.image.isEmpty
        ? String(localized: "Choose how the screen should look")
        : String(localized: "Screen matches \(imageName(reference.image))")
    case .if(let reference, _, let elseIf, _):
      let first =
        if let expect = expectation(reference) {
          String(localized: "If the screen shows \(quoted(expect, limit: 40))")
        } else if reference.image.isEmpty {
          String(localized: "Choose what to look for")
        } else {
          String(localized: "If \(imageName(reference.image)) is on screen")
        }
      return elseIf.isEmpty ? first : first + " · " + String(localized: "or \(elseIf.count) more")
    case .repeat(let times, _, _):
      return String(localized: "\(Int(times)) times")
    case .useData(let file, _, _):
      return file.isEmpty
        ? String(localized: "Choose a CSV file")
        : String(localized: "For each row of \((file as NSString).lastPathComponent)")
    case .runTest(let test, _):
      return test.isEmpty
        ? String(localized: "Choose a test")
        : String(localized: "Runs \(quoted(ProjectScanner.testName((test as NSString).lastPathComponent)))")
    }
  }

  /// `refs/sign-in-button.png` → “sign in button”.
  static func imageName(_ path: String) -> String {
    let base = ((path as NSString).lastPathComponent as NSString).deletingPathExtension
    return quoted(base.replacing(/[-_]+/, with: " ").trimmingCharacters(in: .whitespaces))
  }

  /// On one line: Return and Tab show as the symbols on their keys. The quotation marks are those of
  /// the language the app speaks: “…” in English, «…» in Russian.
  static func quoted(_ text: String, limit: Int = 24) -> String {
    let line = String(text.map { $0.isNewline ? "⏎" : $0 == "\t" ? "⇥" : $0 })
    let short = line.count > limit ? line.prefix(limit - 1) + "…" : line
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
    }
  }

  private static func referenceName(_ reference: Reference) -> String {
    if let expect = expectation(reference) { return quoted(expect, limit: 40) }
    return reference.image.isEmpty ? String(localized: "an image") : imageName(reference.image)
  }

  /// What a comparison by meaning expects, when it says.
  private static func expectation(_ reference: Reference) -> String? {
    guard case .meaning(let expect) = reference.compare else { return nil }
    let text = expect.trimmingCharacters(in: .whitespacesAndNewlines)
    return text.isEmpty ? nil : text
  }

  static func percent(_ fraction: Double) -> String {
    fraction.formatted(.percent.precision(.fractionLength(0...1)))
  }
}
