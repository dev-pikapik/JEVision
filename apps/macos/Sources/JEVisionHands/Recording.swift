import CoreGraphics
import Foundation
import JEVisionCore

/// The front window of the app under test, as the recorder saw it at one moment.
public struct WindowFrame: Sendable {
  public let window: CGWindowID
  /// Where the window is, in global points from the top-left corner of the main display.
  public let bounds: CGRect
  /// Pixels per point.
  public let scale: Double
  public let title: String?
  public let bitmap: Bitmap

  public init(window: CGWindowID, bounds: CGRect, scale: Double, title: String?, bitmap: Bitmap) {
    self.window = window
    self.bounds = bounds
    self.scale = scale
    self.title = title
    self.bitmap = bitmap
  }
}

/// What Accessibility tells about the control a person clicked or typed into.
public struct ElementInfo: Sendable, Equatable {
  public var role: String?
  public var subrole: String?
  /// What a person would call it: its title, description or placeholder.
  public var label: String?
  /// Global points, top-left origin.
  public var frame: CGRect?

  public init(role: String? = nil, subrole: String? = nil, label: String? = nil, frame: CGRect? = nil) {
    self.role = role
    self.subrole = subrole
    self.label = label
    self.frame = frame
  }

  public var isSecure: Bool { subrole == "AXSecureTextField" }
  var takesText: Bool { isSecure || ["AXTextField", "AXTextArea", "AXComboBox"].contains(role ?? "") }
}

/// One key press seen while recording.
public struct KeyPress: Sendable {
  public struct Modifiers: OptionSet, Sendable {
    public let rawValue: Int
    public init(rawValue: Int) { self.rawValue = rawValue }
    public static let command = Modifiers(rawValue: 1)
    public static let shift = Modifiers(rawValue: 2)
    public static let option = Modifiers(rawValue: 4)
    public static let control = Modifiers(rawValue: 8)
  }

  public let code: CGKeyCode
  /// What the key typed with the current layout; empty for keys that type nothing.
  public let characters: String
  public let modifiers: Modifiers
  public let isRepeat: Bool
  /// Seconds since the Mac started.
  public let time: TimeInterval

  public init(code: CGKeyCode, characters: String, modifiers: Modifiers = [], isRepeat: Bool = false, time: TimeInterval = 0) {
    self.code = code
    self.characters = characters
    self.modifiers = modifiers
    self.isRepeat = isRepeat
    self.time = time
  }
}

/// A picture the recording keeps: a click target, a state to wait for, a reference to check.
public struct Picture: Sendable {
  public let bitmap: Bitmap
  /// The file name to suggest, without extension: `save-button`.
  public let name: String
}

public enum ClickTarget: Sendable {
  case image(Picture)
  /// Fractions of the window, for places that look like others, such as a spot in an empty page.
  case point(Point)
}

/// One step of a recording; becomes one block.
public enum RecordedStep: Sendable {
  case openApp
  case click(ClickTarget, button: MouseButton, double: Bool)
  case type(String, into: ClickTarget?)
  /// Typing into a password field: the test types the secret, the recording never keeps the text.
  case secret(String, into: ClickTarget?)
  case pressKeys(String)
  /// The whole window, as it looked when the next step began.
  case waitFor(Picture)
  case check(Picture, area: Rect)
}

/// Turns what a person does in the app under test into the steps of a test: clicks with a picture
/// of what was clicked, typed text joined into one step, shortcuts, waits for each new screen.
/// Pure: the recorder feeds it events and pictures, tests feed it made-up ones.
public struct Recording: Sendable {
  /// How alike a click target must be to count as found: the runner's own threshold.
  public static let minSimilarity = 0.95
  /// How long a recorded wait lasts at most.
  public static let waitTimeoutMs: UInt64 = 10_000
  /// Share of a window that must change for the next step to wait for the new look.
  static let changedShare = 0.2

  public private(set) var steps: [RecordedStep] = []
  /// Names of the secrets the recording types.
  public var secrets: [String] {
    steps.compactMap { if case .secret(let name, _) = $0 { name } else { nil } }
  }
  /// The steps a person made, without the opening one.
  public var isEmpty: Bool { !steps.contains { if case .openApp = $0 { false } else { true } } }

  private let appName: String
  /// Typed text goes on into the last step.
  private var typing = false
  /// The last step clicked into a text field, so typing that follows joins it.
  private var clickedField = false
  /// The window when the last step began, to notice a new screen.
  private var previous: WindowFrame?
  private var escape = EscapeDetector()
  /// The Esc press just recorded, while it may still be the first of Esc Esc.
  private var escapeStep: Int?

  /// `opensApp` starts the test by opening the app; `appName` goes into the names of secrets.
  public init(appName: String, opensApp: Bool) {
    self.appName = appName
    if opensApp { steps = [.openApp] }
  }

  public enum ClickResult: Equatable, Sendable {
    case recorded
    /// Outside the app's front window (its menus, say): a test can't click there.
    case outsideWindow
    case ignored
  }

  /// A mouse press at `point` (global points). `inApp`: it went to the app under test, not to
  /// another app or to JEVision's own panels. `frame` is the window before the click.
  public mutating func click(
    at point: CGPoint, button: MouseButton, count: Int, inApp: Bool, frame: WindowFrame?, element: ElementInfo?
  ) -> ClickResult {
    guard inApp else { return .ignored }
    escapeStep = nil
    guard let frame, frame.bounds.contains(point) else { return .outsideWindow }
    if count == 2, case .click(let target, button, false) = steps.last, !typing {
      steps[steps.count - 1] = .click(target, button: button, double: true)
      clickedField = false
      return .recorded
    }
    guard count == 1 else { return .ignored }
    settle(frame)
    let target = Self.target(at: point, in: frame, element: element)
    if let element, element.isSecure, button == .left {
      steps.append(.secret(secretName(element), into: target))
    } else {
      steps.append(.click(target, button: button, double: false))
    }
    typing = false
    clickedField = button == .left && (element?.takesText ?? false)
    return .recorded
  }

  public enum KeyResult: Equatable, Sendable {
    case recorded
    case ignored
    /// Esc Esc: the person wants to stop recording.
    case stop
  }

  /// A key press; Esc Esc stops in any app, other keys count only `inApp`. `frame` is the window
  /// before the press, `focus` the control that has the keyboard, when known.
  public mutating func key(_ press: KeyPress, inApp: Bool, frame: WindowFrame?, focus: ElementInfo?) -> KeyResult {
    let isEscape = Int64(press.code) == KeyStroke.escapeKeyCode && press.modifiers.isEmpty
    if escape.keyDown(isEscape: isEscape, isRepeat: press.isRepeat, at: press.time) {
      if let index = escapeStep, index == steps.count - 1 { dropLast() }
      escapeStep = nil
      return .stop
    }
    guard inApp else { return .ignored }
    escapeStep = nil
    if let focus, focus.isSecure {
      focusMoved(to: focus, frame: frame)
      return .ignored
    }
    let name = KeyStroke.names[press.code]
    let shortcut = !press.modifiers.isDisjoint(with: [.command, .control])
    let text: String? =
      switch press.code {
      case 36, 76: "\n"
      case 48: "\t"
      case 51: nil
      default: name.map { Self.specialKeys.contains($0) } == true ? nil : Self.printable(press.characters)
      }
    if !shortcut, press.code == 51, typing, case .type(var typed, let into) = steps.last {
      typed.removeLast()
      if !typed.isEmpty {
        steps[steps.count - 1] = .type(typed, into: into)
      } else if let into {
        steps[steps.count - 1] = .click(into, button: .left, double: false)
        clickedField = true
        typing = false
      } else {
        steps.removeLast()
        typing = false
      }
      return .recorded
    }
    // Return and Tab are text while typing, keys of their own otherwise.
    if !shortcut, let text, typing || clickedField || !["\n", "\t"].contains(text) {
      if case .secret = steps.last {
        // The keys went into the password field unless the keyboard is known to be elsewhere.
        // macOS hides keys pressed in a password field, Tab out of it included, so a field that
        // took the keyboard since is clicked instead.
        guard let focus, let control = focus.frame, let frame, frame.bounds.contains(CGPoint(x: control.midX, y: control.midY))
        else { return .ignored }
        settle(frame)
        steps.append(.type(text, into: Self.target(at: CGPoint(x: control.midX, y: control.midY), in: frame, element: focus)))
      } else if typing, case .type(let typed, let into) = steps.last {
        steps[steps.count - 1] = .type(typed + text, into: into)
      } else if clickedField, case .click(let target, .left, false) = steps.last {
        steps[steps.count - 1] = .type(text, into: target)
      } else {
        settle(frame)
        steps.append(.type(text, into: nil))
      }
      typing = true
      clickedField = false
      return .recorded
    }
    guard !press.isRepeat, let name else { return .ignored }
    var keys: [String] = []
    if press.modifiers.contains(.command) { keys.append("cmd") }
    if press.modifiers.contains(.shift) { keys.append("shift") }
    if press.modifiers.contains(.option) { keys.append("option") }
    if press.modifiers.contains(.control) { keys.append("ctrl") }
    keys.append(name)
    settle(frame)
    steps.append(.pressKeys(keys.joined(separator: "+")))
    typing = false
    clickedField = false
    if isEscape { escapeStep = steps.count - 1 }
    return .recorded
  }

  /// The keyboard moved to `focus`, by Tab or by the app itself. A password field there becomes a
  /// secret: macOS hides what is typed into it, from the recorder too.
  public mutating func focusMoved(to focus: ElementInfo, frame: WindowFrame?) {
    guard focus.isSecure else { return }
    if case .secret = steps.last { return }
    settle(frame)
    steps.append(.secret(secretName(focus), into: nil))
    typing = false
    clickedField = false
  }

  /// A check that `area` (global points) of the window looks as it does in `frame`.
  public mutating func check(area: CGRect, frame: WindowFrame, label: String?) -> Bool {
    let part = area.intersection(frame.bounds)
    guard !part.isNull, part.width >= 4, part.height >= 4 else { return false }
    settle(frame)
    let b = frame.bounds
    let rect = Rect(
      x: (part.minX - b.minX) / b.width, y: (part.minY - b.minY) / b.height,
      width: part.width / b.width, height: part.height / b.height)
    steps.append(.check(Picture(bitmap: frame.bitmap, name: Self.slug("check " + (label ?? ""))), area: rect))
    typing = false
    clickedField = false
    escapeStep = nil
    return true
  }

  /// Takes back the last step, and the wait that came with it.
  public mutating func undoLastStep() {
    guard let last = steps.last else { return }
    if case .openApp = last { return }
    dropLast()
    typing = false
    clickedField = false
    escapeStep = nil
    // The app is still showing what the step did; the next step decides afresh.
    previous = nil
  }

  private mutating func dropLast() {
    steps.removeLast()
    if case .waitFor = steps.last { steps.removeLast() }
  }

  /// A new step begins: if the window is not the one the last step began in, wait for it first.
  private mutating func settle(_ frame: WindowFrame?) {
    guard let frame else { return }
    defer { previous = frame }
    guard let previous, Self.isNewScreen(from: previous, to: frame) else { return }
    if case .secret = steps.last {
      // A new screen right after a password usually means Return sent the form; the
      // hidden key can't be seen, so a form sent some other way gets an extra Return.
      steps.append(.pressKeys("return"))
    }
    steps.append(.waitFor(Picture(bitmap: frame.bitmap, name: Self.slug(frame.title ?? "", or: "screen"))))
  }

  // MARK: Pictures

  /// Another window, a new size, or a fifth of the window changed.
  static func isNewScreen(from a: WindowFrame, to b: WindowFrame) -> Bool {
    let (x, y) = (a.bitmap, b.bitmap)
    guard a.window == b.window, x.width == y.width, x.height == y.height else { return true }
    var changed = 0, total = 0
    x.pixels.withUnsafeBufferPointer { p in
      y.pixels.withUnsafeBufferPointer { q in
        for row in stride(from: 0, to: x.height, by: 3) {
          for column in stride(from: 0, to: x.width, by: 3) {
            let i = (row * x.width + column) * 4
            total += 1
            if abs(Int(p[i]) - Int(q[i])) > TemplateMatcher.tolerance || abs(Int(p[i + 1]) - Int(q[i + 1])) > TemplateMatcher.tolerance
              || abs(Int(p[i + 2]) - Int(q[i + 2])) > TemplateMatcher.tolerance
            {
              changed += 1
            }
          }
        }
      }
    }
    return Double(changed) > Double(total) * changedShare
  }

  /// What to click at `point`: the smallest picture around it that shows up nowhere else in the
  /// window, starting from the control itself; a place in the window when every picture repeats.
  static func target(at point: CGPoint, in frame: WindowFrame, element: ElementInfo?) -> ClickTarget {
    let image = frame.bitmap, scale = frame.scale
    let px = Int(((point.x - frame.bounds.minX) * scale).rounded(.down))
    let py = Int(((point.y - frame.bounds.minY) * scale).rounded(.down))
    var places: [(x: Int, y: Int, width: Int, height: Int)] = []
    // A control that fills much of the window, like a page of text, says little about the spot.
    let control = element?.frame.flatMap {
      $0.contains(point) && $0.width * $0.height * 8 <= frame.bounds.width * frame.bounds.height ? $0 : nil
    }
    if let control {
      let x = Int(((control.minX - frame.bounds.minX) * scale).rounded())
      let y = Int(((control.minY - frame.bounds.minY) * scale).rounded())
      let width = Int((control.width * scale).rounded()), height = Int((control.height * scale).rounded())
      for pad in [0, 8, 24] {
        let p = Int(Double(pad) * scale)
        places.append((x - p, y - p, width + 2 * p, height + 2 * p))
      }
    }
    for side in [32.0, 48, 72, 108, 160, 240] {
      let s = Int(side * scale)
      places.append((px - s / 2, py - s / 2, s, s))
    }
    let name = Self.name(of: element)
    for place in places {
      // Kept inside the window, shifted rather than cut where it sticks out.
      let width = min(place.width, image.width), height = min(place.height, image.height)
      guard width >= 8, height >= 8, width * 2 <= image.width || height * 2 <= image.height else { continue }
      let x = min(max(place.x, 0), image.width - width), y = min(max(place.y, 0), image.height - height)
      // The test clicks the middle of the picture: it must land on the same control, or close by.
      let middle = CGPoint(x: Double(x + width / 2) / scale + frame.bounds.minX, y: Double(y + height / 2) / scale + frame.bounds.minY)
      guard abs(middle.x - point.x) <= 4 && abs(middle.y - point.y) <= 4 || control?.contains(middle) == true else { continue }
      let cut = image.crop(x: x, y: y, width: width, height: height)
      if isUnique(cut, at: (x, y), in: image) { return .image(Picture(bitmap: cut, name: name)) }
    }
    return .point(
      Point(
        x: (point.x - frame.bounds.minX) / frame.bounds.width, y: (point.y - frame.bounds.minY) / frame.bounds.height))
  }

  /// Whether the runner's search finds `template` at `place` and nowhere else.
  static func isUnique(_ template: Bitmap, at place: (x: Int, y: Int), in image: Bitmap) -> Bool {
    guard let found = TemplateMatcher.find(template, in: image, minSimilarity: minSimilarity),
      abs(found.x - place.x) <= 1, abs(found.y - place.y) <= 1
    else { return false }
    // Spoil the place itself: whatever matches now is a look-alike elsewhere.
    var pixels = image.pixels
    for row in place.y..<place.y + template.height {
      for i in (row * image.width + place.x) * 4..<(row * image.width + place.x + template.width) * 4 where i % 4 != 3 {
        pixels[i] &+= 128
      }
    }
    let spoiled = Bitmap(width: image.width, height: image.height, pixels: pixels)
    return TemplateMatcher.find(template, in: spoiled, minSimilarity: minSimilarity) == nil
  }

  // MARK: Names

  /// `save-button` for a button titled Save.
  static func name(of element: ElementInfo?) -> String {
    let kinds = [
      "AXButton": "button", "AXTextField": "field", "AXTextArea": "text", "AXComboBox": "field",
      "AXCheckBox": "checkbox", "AXRadioButton": "option", "AXPopUpButton": "menu", "AXMenuButton": "menu",
      "AXLink": "link", "AXTab": "tab", "AXRow": "row", "AXCell": "cell", "AXImage": "image",
      "AXStaticText": "text", "AXSlider": "slider", "AXDisclosureTriangle": "disclosure",
    ]
    let kind = element?.role.flatMap { kinds[$0] }
    let label = element?.label ?? ""
    let words = label.lowercased().split(separator: " ").map(String.init)
    let suffix = kind.map { words.last == $0 ? "" : " " + $0 } ?? ""
    return slug(label + suffix, or: kind ?? "click")
  }

  /// `Confirm password` in Acme Mail → `ACME_MAIL_CONFIRM_PASSWORD`.
  func secretName(_ field: ElementInfo) -> String {
    let app = Self.slug(appName).replacing("-", with: "_").uppercased()
    var label = Self.slug(field.label ?? "").replacing("-", with: "_").uppercased()
    if !label.contains("PASSWORD") { label = label.isEmpty ? "PASSWORD" : label + "_PASSWORD" }
    let name = app.isEmpty ? label : app + "_" + label
    return name.first?.isNumber == true ? "_" + name : name
  }

  /// Lower-case Latin letters and digits joined by dashes, at most 40 characters.
  static func slug(_ text: String, or fallback: String = "picture") -> String {
    let latin = text.applyingTransform(.toLatin, reverse: false)?.applyingTransform(.stripDiacritics, reverse: false) ?? text
    var out = ""
    for c in latin.lowercased() {
      if c.isASCII, c.isLetter || c.isNumber {
        out.append(c)
      } else if !out.isEmpty, out.last != "-" {
        out.append("-")
      }
    }
    let trimmed = String(out.prefix(40)).trimmingCharacters(in: CharacterSet(charactersIn: "-"))
    return trimmed.isEmpty ? fallback : trimmed
  }

  // MARK: Keys

  /// Keys that move or edit rather than type.
  static let specialKeys: Set<String> = [
    "escape", "forwarddelete", "home", "end", "pageup", "pagedown", "left", "right", "down", "up",
    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12", "f13", "f14", "f15",
    "f16", "f17", "f18", "f19", "f20",
  ]

  /// The text without control characters, or `nil` when nothing is left.
  static func printable(_ characters: String) -> String? {
    let text = String(characters.unicodeScalars.filter { !CharacterSet.controlCharacters.contains($0) }.map(Character.init))
    return text.isEmpty ? nil : text
  }
}

extension Bitmap {
  func crop(x: Int, y: Int, width: Int, height: Int) -> Bitmap {
    var out = [UInt8]()
    out.reserveCapacity(width * height * 4)
    for row in y..<y + height {
      let start = (row * self.width + x) * 4
      out.append(contentsOf: pixels[start..<start + width * 4])
    }
    return Bitmap(width: width, height: height, pixels: out)
  }
}

extension Recording {
  /// The blocks for `steps`, their pictures saved as PNG files in the `refs` folder next to the
  /// test, each under a name no other file there has; and those files. When saving fails, none
  /// of them is left behind.
  public static func blocks(for steps: [RecordedStep], testFolder: URL) throws -> (blocks: [BlockKind], files: [URL]) {
    let folder = testFolder.appending(path: "refs", directoryHint: .isDirectory)
    var taken = Set<String>()
    var files: [URL] = []
    func save(_ picture: Picture) throws -> String {
      try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
      var name = picture.name, number = 1
      while taken.contains(name) || FileManager.default.fileExists(atPath: folder.appending(path: name + ".png").path) {
        number += 1
        name = "\(picture.name)-\(number)"
      }
      guard let png = picture.bitmap.png else { throw CocoaError(.fileWriteUnknown) }
      let file = folder.appending(path: name + ".png")
      try png.write(to: file, options: .withoutOverwriting)
      files.append(file)
      taken.insert(name)
      return "refs/\(name).png"
    }
    do {
      return (try steps.map { try kind($0, picture: save) }, files)
    } catch {
      for file in files { try? FileManager.default.removeItem(at: file) }
      throw error
    }
  }

  /// The block `step` becomes, its pictures not saved yet: to show while recording.
  public static func preview(_ step: RecordedStep) -> BlockKind {
    kind(step) { "refs/\($0.name).png" }
  }

  private static func kind(_ step: RecordedStep, picture path: (Picture) throws -> String) rethrows -> BlockKind {
    func target(_ t: ClickTarget) throws -> Target {
      switch t {
      case .image(let picture): .image(try path(picture))
      case .point(let point): .point(point)
      }
    }
    func reference(_ picture: Picture, area: Rect? = nil) throws -> Reference {
      Reference(image: try path(picture), compare: .similar(threshold: minSimilarity), area: area, ignore: [], hide: [])
    }
    return switch step {
    case .openApp: .openApp(app: nil, next: nil)
    case .click(let t, let button, let double):
      .click(target: try target(t), button: button, double: double, waitForChange: false, next: nil)
    case .type(let text, let into): .type(text: text, into: try into.map(target), next: nil)
    case .secret(let name, let into): .type(text: "${secret.\(name)}", into: try into.map(target), next: nil)
    case .pressKeys(let keys): .pressKeys(keys: keys, next: nil)
    case .waitFor(let picture): .waitFor(until: .appears(try reference(picture)), timeoutMs: waitTimeoutMs, next: nil)
    case .check(let picture, let area): .check(reference: try reference(picture, area: area), next: nil)
    }
  }
}

extension Bitmap {
  /// The pixel values as they are, in a PNG file.
  var png: Data? {
    let space = CGColorSpace(name: CGColorSpace.sRGB)!
    let image = pixels.withUnsafeBytes { buffer -> CGImage? in
      guard let provider = CGDataProvider(data: Data(buffer) as CFData) else { return nil }
      return CGImage(
        width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4, space: space,
        bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue), provider: provider,
        decode: nil, shouldInterpolate: false, intent: .defaultIntent)
    }
    return image.flatMap(MacHands.png)
  }
}
