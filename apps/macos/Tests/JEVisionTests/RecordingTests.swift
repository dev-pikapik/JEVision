import CoreGraphics
import Foundation
import JEVisionCore
import Testing

@testable import JEVision
@testable import JEVisionHands

/// Made-up streams of clicks and keys, the way the recorder would feed them.
struct RecordingTests {
  static let bounds = CGRect(x: 100, y: 50, width: 200, height: 120)

  /// A window of 8-pixel tiles, each a colour of its own: every part of it looks unique.
  static func tiles(window: CGWindowID = 1, title: String? = "Untitled", shift: Int = 0) -> WindowFrame {
    let (width, height) = (Int(bounds.width), Int(bounds.height))
    var pixels = [UInt8](repeating: 255, count: width * height * 4)
    for y in 0..<height {
      for x in 0..<width {
        let i = (y * width + x) * 4
        let (cx, cy) = (x / 8 + shift, y / 8)
        pixels[i] = UInt8((cx * 37 + cy * 11) % 256)
        pixels[i + 1] = UInt8((cy * 53 + cx * 7) % 256)
        pixels[i + 2] = UInt8((cx * cy * 29 + 90) % 256)
      }
    }
    return frame(Bitmap(width: width, height: height, pixels: pixels), window: window, title: title)
  }

  /// A window of the same two tiles over and over: no part of it is unique.
  static func checkerboard() -> WindowFrame {
    let (width, height) = (Int(bounds.width), Int(bounds.height))
    var pixels = [UInt8](repeating: 255, count: width * height * 4)
    for y in 0..<height {
      for x in 0..<width where (x / 8 + y / 8) % 2 == 0 {
        let i = (y * width + x) * 4
        pixels[i] = 20
        pixels[i + 1] = 40
        pixels[i + 2] = 60
      }
    }
    return frame(Bitmap(width: width, height: height, pixels: pixels), window: 1, title: nil)
  }

  static func frame(_ bitmap: Bitmap, window: CGWindowID, title: String?) -> WindowFrame {
    WindowFrame(window: window, bounds: bounds, scale: 1, title: title, bitmap: bitmap)
  }

  /// The steps in a few words each, to compare.
  static func describe(_ steps: [RecordedStep]) -> [String] {
    func target(_ t: ClickTarget?) -> String {
      switch t {
      case .image(let picture): " @" + picture.name
      case .point: " @point"
      case nil: ""
      }
    }
    return steps.map { step in
      switch step {
      case .openApp: "open"
      case .click(let t, let button, let double): (double ? "double " : "") + (button == .right ? "right " : "") + "click" + target(t)
      case .drag(let from, let to): "drag" + target(from) + " to" + target(to)
      case .scroll(let direction, let amount, let unit, let at): "scroll \(direction) \(amount) \(unit) at \(at.x) \(at.y)"
      case .type(let text, let into): "type \(text)" + target(into)
      case .secret(let name, let into): "secret \(name)" + target(into)
      case .pressKeys(let keys): "keys \(keys)"
      case .waitFor(let picture): "wait \(picture.name)"
      case .check(let picture, _): "check \(picture.name)"
      }
    }
  }

  static func press(_ key: String, _ modifiers: KeyPress.Modifiers = [], text: String? = nil, at time: TimeInterval = 0, isRepeat: Bool = false)
    -> KeyPress
  {
    let code = key == "enter" ? 76 : KeyStroke.keys[key]!
    let characters = text ?? (key.count == 1 ? (modifiers.contains(.shift) ? key.uppercased() : key) : "")
    return KeyPress(code: code, characters: characters, modifiers: modifiers, isRepeat: isRepeat, time: time)
  }

  let field = ElementInfo(role: "AXTextField", label: "Name", frame: CGRect(x: 130, y: 70, width: 80, height: 20))
  let password = ElementInfo(
    role: "AXTextField", subrole: "AXSecureTextField", label: "Password", frame: CGRect(x: 130, y: 100, width: 80, height: 20))

  @Test func joinsTypedTextAndFixesItWithBackspace() {
    var r = Recording(appName: "TextEdit", opensApp: true)
    let frame = Self.tiles()
    for key in ["h", "e", "l", "x", "delete", "l", "o", "space"] {
      _ = r.key(Self.press(key, text: key == "space" ? " " : nil), inApp: true, frame: frame, focus: nil)
    }
    _ = r.key(Self.press("w", [.shift]), inApp: true, frame: frame, focus: nil)
    #expect(Self.describe(r.steps) == ["open", "type hello W"])
    // Backspace through all of it takes the step back.
    for _ in 0..<7 { _ = r.key(Self.press("delete"), inApp: true, frame: frame, focus: nil) }
    #expect(Self.describe(r.steps) == ["open"])
    #expect(r.isEmpty)
  }

  @Test func typingRightAfterClickingAFieldTypesIntoIt() {
    var r = Recording(appName: "Mail", opensApp: false)
    let frame = Self.tiles()
    #expect(r.click(at: CGPoint(x: 150, y: 80), button: .left, count: 1, inApp: true, frame: frame, element: field) == .recorded)
    _ = r.key(Self.press("a"), inApp: true, frame: frame, focus: field)
    _ = r.key(Self.press("return"), inApp: true, frame: frame, focus: field)
    #expect(Self.describe(r.steps) == ["type a\n @name-field"])
    // Erasing the text leaves the click.
    _ = r.key(Self.press("delete"), inApp: true, frame: frame, focus: field)
    _ = r.key(Self.press("delete"), inApp: true, frame: frame, focus: field)
    #expect(Self.describe(r.steps) == ["click @name-field"])
  }

  @Test func passwordBecomesASecretAndItsTextIsNeverKept() {
    var r = Recording(appName: "Acme Mail", opensApp: false)
    let frame = Self.tiles()
    _ = r.click(at: CGPoint(x: 150, y: 110), button: .left, count: 1, inApp: true, frame: frame, element: password)
    for key in ["s", "e", "c", "r", "e", "t"] {
      #expect(r.key(Self.press(key), inApp: true, frame: frame, focus: password) == .ignored)
    }
    // Tab out is hidden too; the next field is known by what has the keyboard.
    _ = r.key(Self.press("tab"), inApp: true, frame: frame, focus: password)
    _ = r.key(Self.press("j"), inApp: true, frame: frame, focus: field)
    _ = r.key(Self.press("o"), inApp: true, frame: frame, focus: field)
    #expect(Self.describe(r.steps) == ["secret ACME_MAIL_PASSWORD @password-field", "type jo @name-field"])
    #expect(r.secrets == ["ACME_MAIL_PASSWORD"])
    let typed = r.steps.compactMap { step -> String? in if case .type(let text, _) = step { text } else { nil } }
    #expect(typed == ["jo"])
  }

  @Test func aPasswordFieldThatTakesTheKeyboardBecomesASecret() {
    var r = Recording(appName: "Mail", opensApp: false)
    r.focusMoved(to: password, frame: Self.tiles())
    r.focusMoved(to: password, frame: Self.tiles())
    #expect(Self.describe(r.steps) == ["secret MAIL_PASSWORD"])
  }

  @Test func shortcutsAndSpecialKeysArePressKeys() {
    var r = Recording(appName: "TextEdit", opensApp: false)
    let frame = Self.tiles()
    _ = r.key(Self.press("s", [.command]), inApp: true, frame: frame, focus: nil)
    _ = r.key(Self.press("s", [.command, .shift]), inApp: true, frame: frame, focus: nil)
    _ = r.key(Self.press("z", [.command], isRepeat: true), inApp: true, frame: frame, focus: nil)
    _ = r.key(Self.press("left", text: "\u{F702}"), inApp: true, frame: frame, focus: nil)
    _ = r.key(Self.press("return"), inApp: true, frame: frame, focus: nil)
    _ = r.key(Self.press("tab"), inApp: true, frame: frame, focus: nil)
    #expect(Self.describe(r.steps) == ["keys cmd+s", "keys cmd+shift+s", "keys left", "keys return", "keys tab"])
  }

  @Test func clickKeepsTheSmallestUniquePicture() throws {
    var r = Recording(appName: "TextEdit", opensApp: false)
    let frame = Self.tiles()
    let button = ElementInfo(role: "AXButton", label: "Save", frame: CGRect(x: 200, y: 60, width: 40, height: 16))
    _ = r.click(at: CGPoint(x: 210, y: 65), button: .left, count: 1, inApp: true, frame: frame, element: button)
    _ = r.click(at: CGPoint(x: 160, y: 120), button: .right, count: 1, inApp: true, frame: frame, element: nil)
    guard case .click(.image(let save), _, _) = r.steps[0], case .click(.image(let spot), .right, false) = r.steps[1] else {
      Issue.record("Expected two pictures, got \(Self.describe(r.steps))")
      return
    }
    #expect(save.name == "save-button")
    #expect((save.bitmap.width, save.bitmap.height) == (40, 16))
    #expect(spot.name == "click")
    #expect((spot.bitmap.width, spot.bitmap.height) == (32, 32))
    // The runner finds it where it was cut from, so the test clicks the same place.
    let found = try #require(TemplateMatcher.find(spot.bitmap, in: frame.bitmap, minSimilarity: Recording.minSimilarity))
    #expect((found.x + 16, found.y + 16) == (60, 70))
  }

  @Test func clickOnARepeatingPatternFallsBackToAPlaceInTheWindow() {
    var r = Recording(appName: "TextEdit", opensApp: false)
    _ = r.click(at: CGPoint(x: 150, y: 80), button: .left, count: 1, inApp: true, frame: Self.checkerboard(), element: nil)
    _ = r.click(at: CGPoint(x: 150, y: 80), button: .left, count: 2, inApp: true, frame: Self.checkerboard(), element: nil)
    guard case .click(.point(let point), .left, true) = r.steps.first else {
      Issue.record("Expected a double click on a place, got \(Self.describe(r.steps))")
      return
    }
    #expect(r.steps.count == 1)
    #expect((point.x, point.y) == (0.25, 0.25))
  }

  @Test func waitsForEachNewScreen() {
    var r = Recording(appName: "TextEdit", opensApp: false)
    _ = r.click(at: CGPoint(x: 150, y: 80), button: .left, count: 1, inApp: true, frame: Self.tiles(), element: nil)
    // The same window again: no wait.
    _ = r.key(Self.press("s", [.command]), inApp: true, frame: Self.tiles(), focus: nil)
    // A sheet changed most of it.
    _ = r.key(Self.press("escape"), inApp: true, frame: Self.tiles(shift: 3), focus: nil)
    // Another window.
    _ = r.key(Self.press("w", [.command]), inApp: true, frame: Self.tiles(window: 2, title: "Save Changes?"), focus: nil)
    #expect(Self.describe(r.steps) == ["click @click", "keys cmd+s", "wait untitled", "keys escape", "wait save-changes", "keys cmd+w"])
    // Undo takes the wait along with the step.
    r.undoLastStep()
    #expect(Self.describe(r.steps) == ["click @click", "keys cmd+s", "wait untitled", "keys escape"])
  }

  @Test func movingWithTheButtonDownIsADrag() {
    var r = Recording(appName: "Finder", opensApp: false)
    let frame = Self.tiles()
    _ = r.click(at: CGPoint(x: 150, y: 80), button: .left, count: 1, inApp: true, frame: frame, element: nil)
    // A small slip of the hand stays a click.
    #expect(r.release(at: CGPoint(x: 153, y: 82), element: nil) == .ignored)
    #expect(Self.describe(r.steps) == ["click @click"])
    _ = r.click(at: CGPoint(x: 150, y: 80), button: .left, count: 1, inApp: true, frame: frame, element: nil)
    #expect(r.release(at: CGPoint(x: 250, y: 140), element: nil) == .recorded)
    #expect(Self.describe(r.steps) == ["click @click", "drag @click to @click"])
    // The blocks keep both places.
    guard case .drag(.image, .image, false, nil) = Recording.preview(r.steps[1]) else {
      Issue.record("Expected a drag block, got \(Recording.preview(r.steps[1]))")
      return
    }
    // A drag that ends outside the window can't be repeated.
    _ = r.click(at: CGPoint(x: 150, y: 80), button: .left, count: 1, inApp: true, frame: frame, element: nil)
    #expect(r.release(at: CGPoint(x: 20, y: 10), element: nil) == .outsideWindow)
    #expect(r.steps.count == 2)
    // A right click, or a second click of a double click, is never a drag.
    _ = r.click(at: CGPoint(x: 150, y: 80), button: .right, count: 1, inApp: true, frame: frame, element: nil)
    #expect(r.release(at: CGPoint(x: 250, y: 140), element: nil) == .ignored)
  }

  @Test func turnsOfTheWheelJoinIntoOneScroll() {
    var r = Recording(appName: "TextEdit", opensApp: false)
    let frame = Self.tiles()
    for _ in 0..<3 {
      #expect(r.scroll(at: CGPoint(x: 150, y: 80), vertical: -2, horizontal: 0, pixels: false, inApp: true, frame: frame) == .recorded)
    }
    // The page moved: the next turn is not a new screen to wait for.
    _ = r.scroll(at: CGPoint(x: 150, y: 80), vertical: -1, horizontal: 0, pixels: false, inApp: true, frame: Self.tiles(shift: 3))
    _ = r.scroll(at: CGPoint(x: 150, y: 80), vertical: 1, horizontal: 0, pixels: false, inApp: true, frame: Self.tiles(shift: 3))
    _ = r.scroll(at: CGPoint(x: 200, y: 110), vertical: 3, horizontal: -40, pixels: true, inApp: true, frame: Self.tiles(shift: 3))
    #expect(
      Self.describe(r.steps) == [
        "scroll down 7 lines at 0.25 0.25", "scroll up 1 lines at 0.25 0.25", "scroll right 40 pixels at 0.5 0.5",
      ])
    #expect(r.scroll(at: CGPoint(x: 150, y: 80), vertical: 0, horizontal: 0, pixels: false, inApp: true, frame: frame) == .ignored)
    #expect(r.scroll(at: CGPoint(x: 150, y: 80), vertical: -1, horizontal: 0, pixels: false, inApp: false, frame: frame) == .ignored)
    #expect(r.scroll(at: CGPoint(x: 20, y: 10), vertical: -1, horizontal: 0, pixels: false, inApp: true, frame: frame) == .outsideWindow)
    guard case .scroll(.down, 7, .lines, .point(let at), false, nil) = Recording.preview(r.steps[0]) else {
      Issue.record("Expected a scroll block, got \(Recording.preview(r.steps[0]))")
      return
    }
    #expect((at.x, at.y) == (0.25, 0.25))
  }

  @Test func ignoresInputOutsideTheApp() {
    var r = Recording(appName: "TextEdit", opensApp: true)
    let frame = Self.tiles()
    #expect(r.click(at: CGPoint(x: 150, y: 80), button: .left, count: 1, inApp: false, frame: frame, element: nil) == .ignored)
    #expect(r.key(Self.press("a"), inApp: false, frame: frame, focus: nil) == .ignored)
    #expect(r.click(at: CGPoint(x: 20, y: 10), button: .left, count: 1, inApp: true, frame: frame, element: nil) == .outsideWindow)
    #expect(r.click(at: CGPoint(x: 150, y: 80), button: .left, count: 1, inApp: true, frame: nil, element: nil) == .outsideWindow)
    #expect(r.isEmpty)
  }

  @Test func escapeTwiceStopsAndLeavesNoEscapeBehind() {
    var r = Recording(appName: "TextEdit", opensApp: false)
    let frame = Self.tiles()
    _ = r.key(Self.press("a"), inApp: true, frame: frame, focus: nil)
    #expect(r.key(Self.press("escape", at: 10), inApp: true, frame: frame, focus: nil) == .recorded)
    #expect(r.key(Self.press("escape", at: 10.2), inApp: true, frame: frame, focus: nil) == .stop)
    #expect(Self.describe(r.steps) == ["type a"])
    // From another app, where nothing was recorded.
    #expect(r.key(Self.press("escape", at: 20), inApp: false, frame: frame, focus: nil) == .ignored)
    #expect(r.key(Self.press("escape", at: 20.2), inApp: false, frame: frame, focus: nil) == .stop)
    #expect(Self.describe(r.steps) == ["type a"])
  }

  @Test func checkKeepsTheAreaInSharesOfTheWindow() throws {
    var r = Recording(appName: "TextEdit", opensApp: false)
    let outside = r.check(area: CGRect(x: 0, y: 0, width: 50, height: 50), frame: Self.tiles(), label: nil)
    let inside = r.check(area: CGRect(x: 150, y: 80, width: 100, height: 60), frame: Self.tiles(), label: "Total")
    #expect(!outside && inside)
    guard case .check(let picture, let area) = try #require(r.steps.last) else {
      Issue.record("Expected a check")
      return
    }
    #expect(picture.name == "check-total")
    #expect((area.x, area.y, area.width, area.height) == (0.25, 0.25, 0.5, 0.5))
  }

  @Test func namesReadLikeTheControls() {
    #expect(Recording.slug("Сохранить как…") == "sohranit-kak")
    #expect(Recording.slug("   ") == "picture")
    #expect(Recording.name(of: ElementInfo(role: "AXButton", label: "OK button")) == "ok-button")
    #expect(Recording(appName: "1Password", opensApp: false).secretName(ElementInfo(label: "Master password")) == "_1PASSWORD_MASTER_PASSWORD")
  }

  @Test func writesBlocksAndPicturesNextToTheTest() throws {
    let folder = FileManager.default.temporaryDirectory.appending(path: "recording-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: folder) }
    var r = Recording(appName: "Mail", opensApp: true)
    let frame = Self.tiles()
    let button = ElementInfo(role: "AXButton", label: "Send", frame: CGRect(x: 200, y: 60, width: 40, height: 16))
    _ = r.click(at: CGPoint(x: 210, y: 65), button: .left, count: 1, inApp: true, frame: frame, element: button)
    _ = r.click(at: CGPoint(x: 150, y: 110), button: .left, count: 1, inApp: true, frame: frame, element: password)
    _ = r.click(at: CGPoint(x: 212, y: 66), button: .left, count: 1, inApp: true, frame: Self.tiles(shift: 3), element: button)
    // A new screen right after a password: Return sent the form, though its key was hidden.
    #expect(Self.describe(r.steps) == ["open", "click @send-button", "secret MAIL_PASSWORD @password-field", "keys return", "wait untitled", "click @send-button"])
    let (blocks, saved) = try Recording.blocks(for: r.steps, testFolder: folder)
    try #require(blocks.map(\.typeName) == ["openApp", "click", "type", "pressKeys", "waitFor", "click"])
    guard case .click(.image(let first), _, _, _, _) = blocks[1], case .type(let text, _, _) = blocks[2],
      case .waitFor(.appears(let screen), let timeout, _) = blocks[4], case .click(.image(let second), _, _, _, _) = blocks[5]
    else {
      Issue.record("Unexpected blocks")
      return
    }
    #expect(first == "refs/send-button.png")
    #expect(second == "refs/send-button-2.png")
    #expect(screen.image == "refs/untitled.png")
    #expect(timeout == Recording.waitTimeoutMs)
    #expect(text == "${secret.MAIL_PASSWORD}")
    let files = try FileManager.default.contentsOfDirectory(atPath: folder.appending(path: "refs").path).sorted()
    #expect(files == ["password-field.png", "send-button-2.png", "send-button.png", "untitled.png"])
    #expect(saved.map(\.lastPathComponent).sorted() == files)
    // A second recording doesn't overwrite the first one's pictures.
    let (again, _) = try Recording.blocks(for: [r.steps[1]], testFolder: folder)
    guard case .click(.image(let third), _, _, _, _) = again[0] else { return }
    #expect(third == "refs/send-button-3.png")
  }

  @MainActor @Test func undoingARecordingTrashesItsPictures() throws {
    let folder = FileManager.default.temporaryDirectory.appending(path: "recording-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: folder) }
    let trash = folder.appending(path: "Trash")
    try FileManager.default.createDirectory(at: trash, withIntermediateDirectories: true)
    let url = folder.appending(path: "t.flow.json")
    try #"{"format": 1, "name": "T", "app": "", "start": "a", "blocks": {"a": {"type": "pressKeys", "keys": "a"}}}"#
      .write(to: url, atomically: true, encoding: .utf8)
    var r = Recording(appName: "Mail", opensApp: false)
    let button = ElementInfo(role: "AXButton", label: "Send", frame: CGRect(x: 200, y: 60, width: 40, height: 16))
    _ = r.click(at: CGPoint(x: 210, y: 65), button: .left, count: 1, inApp: true, frame: Self.tiles(), element: button)
    let (blocks, files) = try Recording.blocks(for: r.steps, testFolder: folder)
    try #require(files.count == 1)
    let document = FlowDocument(url: url)
    let undo = UndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    func toTrash(_ file: URL) throws -> URL {
      let target = trash.appending(path: file.lastPathComponent)
      try FileManager.default.moveItem(at: file, to: target)
      return target
    }
    func exists(_ file: URL) -> Bool { FileManager.default.fileExists(atPath: file.path) }

    undo.beginUndoGrouping()
    document.insertRecorded(blocks, files: files, at: Slot(from: "a", edge: "next"), app: "com.apple.mail", trash: toTrash)
    undo.endUndoGrouping()
    #expect(document.flow?.blocks.count == 2)

    undo.undo()
    #expect(document.flow?.blocks.count == 1)
    #expect(document.flow?.app == "")
    #expect(!exists(files[0]))
    #expect(exists(trash.appending(path: files[0].lastPathComponent)))

    undo.redo()
    #expect(document.flow?.blocks.count == 2)
    #expect(exists(files[0]))

    undo.undo()
    #expect(!exists(files[0]))
  }
}
