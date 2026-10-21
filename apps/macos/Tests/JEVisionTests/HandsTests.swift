import AppKit
import CoreGraphics
import CoreMedia
import CoreVideo
import Foundation
import JEVisionCore
import Testing

@testable import JEVisionHands

struct EscapeDetectorTests {
  /// Feeds presses of (key, repeat, time) and returns which of them stopped the run.
  private func stops(_ presses: [(isEscape: Bool, isRepeat: Bool, at: TimeInterval)]) -> [Bool] {
    var detector = EscapeDetector()
    return presses.map { detector.keyDown(isEscape: $0.isEscape, isRepeat: $0.isRepeat, at: $0.at) }
  }

  @Test func stopsOnTwoQuickPresses() {
    // A third press starts a new pair.
    #expect(stops([(true, false, 10), (true, false, 10.3), (true, false, 10.5)]) == [false, true, false])
  }

  @Test func ignoresSlowPressesRepeatsAndOtherKeys() {
    #expect(stops([(true, false, 0), (true, false, EscapeDetector.window + 0.1)]) == [false, false])
    #expect(stops([(true, false, 0), (true, true, 0.1)]) == [false, false])
    #expect(stops([(true, false, 0), (false, false, 0.1), (true, false, 0.2)]) == [false, false, false])
  }
}

/// What a person's mouse and keys do to a run follows the settings, asked at the moment it matters.
struct InputWatcherTests {
  private final class Settings: @unchecked Sendable {
    private let lock = NSLock()
    private var current = RunInputSettings()
    var value: RunInputSettings {
      get { lock.withLock { current } }
      set { lock.withLock { current = newValue } }
    }
  }

  private final class Stops: @unchecked Sendable {
    private let lock = NSLock()
    private var count = 0
    var value: Int { lock.withLock { count } }
    func add() { lock.withLock { count += 1 } }
  }

  private func watcher(_ settings: Settings, _ stops: Stops) -> InputWatcher {
    let watcher = InputWatcher(settings: { settings.value })
    watcher.arm(onEmergencyStop: { stops.add() })
    return watcher
  }

  private func escapeTwice(_ watcher: InputWatcher, at time: TimeInterval) {
    watcher.note(key: (isEscape: true, isRepeat: false), at: time)
    watcher.note(key: (isEscape: true, isRepeat: false), at: time + 0.2)
  }

  @Test func theMouseAndKeysPauseTheRunUnlessSettingsSayNot() {
    let settings = Settings()
    let watcher = watcher(settings, Stops())
    watcher.note(key: nil, at: 0)
    #expect(watcher.takeUserInput())
    #expect(!watcher.takeUserInput())
    settings.value.pausesOnMouse = false
    watcher.note(key: nil, at: 1)
    #expect(!watcher.takeUserInput())
    // Turned back on, what happened meanwhile is not held against the person.
    settings.value.pausesOnMouse = true
    #expect(!watcher.takeUserInput())
    watcher.note(key: (isEscape: false, isRepeat: false), at: 2)
    #expect(watcher.takeUserInput())
  }

  @Test func escapeTwiceStopsTheRunUnlessSettingsSayNotAndAtOnce() {
    let settings = Settings()
    let stops = Stops()
    let watcher = watcher(settings, stops)
    escapeTwice(watcher, at: 0)
    #expect(stops.value == 1)
    settings.value.escapeStops = false
    escapeTwice(watcher, at: 10)
    #expect(stops.value == 1)
    settings.value.escapeStops = true
    escapeTwice(watcher, at: 20)
    #expect(stops.value == 2)
  }

  @Test func settingsReadTheChoicesAndKeepTheDefaultsWhenThereIsNone() throws {
    let name = "InputWatcherTests.\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: name))
    defer { defaults.removePersistentDomain(forName: name) }
    #expect(RunInputSettings(defaults) == RunInputSettings())
    defaults.set(false, forKey: RunInputSettings.pausesOnMouseKey)
    defaults.set(false, forKey: RunInputSettings.collapsesKey)
    let settings = RunInputSettings(defaults)
    #expect(!settings.pausesOnMouse && !settings.collapsesToPanel && settings.escapeStops)
  }
}

struct SyntheticInputTests {
  @Test func tellsTheTestsInputFromAPersons() throws {
    let own = try #require(CGEvent(mouseEventSource: SyntheticInput.source, mouseType: .mouseMoved, mouseCursorPosition: .zero, mouseButton: .left))
    #expect(SyntheticInput.isSynthetic(own))
    let person = try #require(
      CGEvent(keyboardEventSource: CGEventSource(stateID: .hidSystemState), virtualKey: 53, keyDown: true))
    #expect(!SyntheticInput.isSynthetic(person))
  }
}

struct WindowCoordinateTests {
  @Test func pointsInTheWindowFollowItToAnyDisplay() {
    // A display left of the main one has negative x; one above it, negative y.
    let left = CGRect(x: -1440, y: 80, width: 900, height: 600)
    #expect(MacHands.screenPoint(Point(x: 10, y: 20), in: left) == CGPoint(x: -1430, y: 100))
    let above = CGRect(x: 200, y: -900, width: 900, height: 600)
    #expect(MacHands.screenPoint(Point(x: 450, y: 300), in: above) == CGPoint(x: 650, y: -600))
  }

  @Test func theCaptureCropsToTheWindowOfItsOwnDisplay() {
    let display = CGRect(x: -1440, y: 0, width: 1440, height: 900)
    let window = CGRect(x: -1300, y: 120, width: 800, height: 500)
    #expect(MacHands.sourceRect(of: window, on: display) == CGRect(x: 140, y: 120, width: 800, height: 500))
  }
}

struct WindowPlacementTests {
  private let size = WindowSize(width: 900, height: 600)
  private let position = WindowPosition(x: 40, y: 60)

  @Test func aWindowAsWantedNeedsNothing() {
    let frame = CGRect(x: 40, y: 60, width: 900, height: 600)
    #expect(WindowPlacement.difference(size: size, position: position, frame: frame) == nil)
    // Apps round to whole points.
    let rounded = CGRect(x: 41, y: 59.4, width: 899.2, height: 601)
    #expect(WindowPlacement.difference(size: size, position: position, frame: rounded) == nil)
    // Nothing wanted, nothing to differ.
    #expect(WindowPlacement.difference(size: nil, position: nil, frame: .zero) == nil)
  }

  @Test func aWindowThatKeptItsOwnLimitsSaysWhat() throws {
    let narrow = CGRect(x: 40, y: 60, width: 900, height: 480)
    let wrongSize = try #require(WindowPlacement.difference(size: size, position: position, frame: narrow))
    #expect(wrongSize == (what: "size", wanted: "900 × 600", actual: "900 × 480"))
    // Only what was asked for is checked.
    #expect(WindowPlacement.difference(size: nil, position: position, frame: narrow) == nil)

    let pushedDown = CGRect(x: 40, y: 25, width: 900, height: 600)
    let wrongPlace = try #require(WindowPlacement.difference(size: size, position: position, frame: pushedDown))
    #expect(wrongPlace == (what: "position", wanted: "40, 60", actual: "40, 25"))
  }

  @Test func aFreshStartTellsTheAppToForgetItsLastWindows() {
    #expect(MacHands.forgetLastSession == ["-ApplePersistenceIgnoreState", "YES"])
  }
}

struct FrontWindowTests {
  @Test func skipsTinyHelperWindows() {
    let helper = (id: CGWindowID(2), bounds: CGRect(x: 447, y: 81, width: 66, height: 20), title: String?("Window"))
    let note = (id: CGWindowID(1), bounds: CGRect(x: 441, y: 75, width: 673, height: 439), title: String?("note.txt"))
    #expect(MacHands.front(of: [helper, note])?.id == 1)
    #expect(MacHands.front(of: [helper])?.id == 2)
    #expect(MacHands.front(of: []) == nil)
  }

  @Test func picksTheAppsWindowsOverTheFrontOne() {
    func info(_ id: Int, pid: Int, _ rect: CGRect) -> [String: Any] {
      [kCGWindowNumber as String: CGWindowID(id), kCGWindowOwnerPID as String: pid_t(pid), kCGWindowBounds as String: rect.dictionaryRepresentation]
    }
    let finder = CGRect(x: 104, y: 237, width: 920, height: 492)
    let list = [
      info(9, pid: 7, CGRect(x: 0, y: 0, width: 3440, height: 24)),  // another app's menu bar
      info(8, pid: 1, CGRect(x: 690, y: 285, width: 303, height: 21)),  // the tooltip
      info(6, pid: 1, CGRect(x: 2000, y: 900, width: 50, height: 50)),  // the app's, elsewhere
      info(5, pid: 1, finder),
      info(4, pid: 1, CGRect(x: 200, y: 300, width: 400, height: 200)),  // behind it
    ]
    #expect(MacHands.overlays(above: 5, bounds: finder, pids: [1], in: list) == [8])
    #expect(MacHands.overlays(above: 8, bounds: finder, pids: [1], in: list) == [])
  }
}

struct KeyStrokeTests {
  @Test func readsCombinations() throws {
    let save = try #require(KeyStroke("cmd+shift+s"))
    #expect(save.key == 1)
    #expect(save.modifiers == [55, 56])
    #expect(save.flags == [.maskCommand, .maskShift])
    #expect(KeyStroke("return")?.modifiers == [])
    #expect(KeyStroke("f12")?.key == 111)
  }

  @Test func rejectsWhatIsntAKey() {
    #expect(KeyStroke("cmd+") == nil)
    #expect(KeyStroke("hyper+a") == nil)
    #expect(KeyStroke("cmd+cmd+a") == nil)
    #expect(KeyStroke("") == nil)
  }
}

struct ModifiersTests {
  @Test func keysAndFlagsAgree() {
    let held = Modifiers(command: true, shift: false, option: true, control: true)
    #expect(held.keyCodes == [55, 58, 59])
    #expect(held.flags == [.maskCommand, .maskAlternate, .maskControl])
    #expect(Modifiers(flags: held.flags) == held)
    #expect(Modifiers(flags: [.maskShift, .maskAlphaShift, .maskNonCoalesced]) == Modifiers(command: false, shift: true, option: false, control: false))
    #expect(held.symbols == "⌃⌥⌘")
    #expect(Modifiers.none.isEmpty && Modifiers.none.keyCodes.isEmpty && Modifiers.none.flags.isEmpty)
  }
}

@Suite(.serialized) struct ClipboardTests {
  @Test func putsTextAndReadsItBack() throws {
    let board = NSPasteboard.general
    let saved = board.string(forType: .string)
    defer {
      board.clearContents()
      if let saved { board.setString(saved, forType: .string) }
    }
    let hands = MacHands(input: InputWatcher(), isCancelled: { false })
    try hands.setClipboard(text: "héllo\nwörld ✓")
    #expect(try hands.clipboard() == "héllo\nwörld ✓")
    try hands.setClipboard(text: "")
    #expect(try hands.clipboard() == "")
    board.clearContents()
    #expect(try hands.clipboard() == "")
  }

  @Test func putsAPictureAndReadsPicturesAndFiles() throws {
    let board = NSPasteboard.general
    let saved = board.string(forType: .string)
    defer {
      board.clearContents()
      if let saved { board.setString(saved, forType: .string) }
    }
    let hands = MacHands(input: InputWatcher(), isCancelled: { false })
    let picture = try #require(
      NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: 3, pixelsHigh: 2, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
        isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0))
    let png = try #require(picture.representation(using: .png, properties: [:]))
    try hands.setClipboardImage(png: png)
    #expect(try hands.clipboardImage().flatMap(NSBitmapImageRep.init(data:))?.pixelsWide == 3)
    #expect(try hands.clipboardFiles().isEmpty)
    #expect(throws: HandsError.self) { try hands.setClipboardImage(png: Data("not a picture".utf8)) }

    // Screenshots put a picture only as TIFF; it still reads as PNG.
    board.clearContents()
    board.setData(picture.tiffRepresentation, forType: .tiff)
    let fromTIFF = try #require(try hands.clipboardImage())
    #expect(fromTIFF.starts(with: [0x89, 0x50, 0x4E, 0x47]))

    let file = FileManager.default.temporaryDirectory.appending(path: "copied file.txt")
    board.clearContents()
    board.writeObjects([file as NSURL])
    #expect(try hands.clipboardFiles() == [file.standardizedFileURL.path])
    #expect(try hands.clipboardImage() == nil)
  }
}

struct MouseButtonTests {
  @Test func theMiddleButtonIsTheOtherButton() {
    let middle = MacHands.events(of: .middle)
    #expect([middle.down, middle.dragged, middle.up] == [.otherMouseDown, .otherMouseDragged, .otherMouseUp])
    #expect(middle.button == .center)
    #expect(MacHands.events(of: .right).down == .rightMouseDown)
    #expect(MacHands.events(of: .left).button == .left)
    #expect(Recorder.button(of: .otherMouseUp) == .middle)
    #expect(Recorder.button(of: .rightMouseDown) == .right)
    #expect(Recorder.button(of: .leftMouseUp) == .left)
  }
}

struct RunLanguageTests {
  @Test func followsTheAppsChoiceOfLanguage() throws {
    let suite = "RunLanguageTests.\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    defer { defaults.removePersistentDomain(forName: suite) }
    defaults.set(["ru-RU"], forKey: "AppleLanguages")
    #expect(RunLanguage.language(defaults) == "ru")
    defaults.set(["de-DE", "en-GB"], forKey: "AppleLanguages")
    #expect(RunLanguage.language(defaults) == "en")
  }
}

struct TemplateMatcherTests {
  /// A noisy picture, so every place looks different.
  private func image(width: Int, height: Int, seed: UInt64 = 7) -> Bitmap {
    var state = seed
    var pixels = [UInt8](repeating: 255, count: width * height * 4)
    for i in stride(from: 0, to: pixels.count, by: 4) {
      for c in 0..<3 {
        state = state &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
        pixels[i + c] = UInt8(truncatingIfNeeded: state >> 56)
      }
    }
    return Bitmap(width: width, height: height, pixels: pixels)
  }

  private func crop(_ b: Bitmap, x: Int, y: Int, width: Int, height: Int) -> Bitmap {
    var pixels: [UInt8] = []
    for row in y..<y + height {
      let start = (row * b.width + x) * 4
      pixels += b.pixels[start..<start + width * 4]
    }
    return Bitmap(width: width, height: height, pixels: pixels)
  }

  @Test func findsTheExactPlace() throws {
    let screen = image(width: 400, height: 300)
    let button = crop(screen, x: 213, y: 141, width: 48, height: 20)
    let match = try #require(TemplateMatcher.find(button, in: screen, minSimilarity: 0.95))
    #expect((match.x, match.y) == (213, 141))
    #expect(match.similarity == 1)
  }

  @Test func respectsTheThreshold() throws {
    let screen = image(width: 200, height: 120)
    var button = crop(screen, x: 50, y: 40, width: 20, height: 20)
    // Change a quarter of the pixels.
    var pixels = button.pixels
    for i in stride(from: 0, to: pixels.count / 4, by: 4) { pixels[i] &+= 128 }
    button = Bitmap(width: 20, height: 20, pixels: pixels)
    let loose = try #require(TemplateMatcher.find(button, in: screen, minSimilarity: 0.7))
    #expect((loose.x, loose.y) == (50, 40))
    #expect(TemplateMatcher.find(button, in: screen, minSimilarity: 0.95) == nil)
  }

  @Test func givesUpOnImpossibleSearches() {
    let screen = image(width: 40, height: 40)
    #expect(TemplateMatcher.find(image(width: 60, height: 10), in: screen, minSimilarity: 0.5) == nil)
    #expect(TemplateMatcher.find(image(width: 10, height: 10, seed: 99), in: screen, minSimilarity: 0.9) == nil)
    #expect(TemplateMatcher.find(crop(screen, x: 0, y: 0, width: 10, height: 10), in: screen, minSimilarity: 0.9, isCancelled: { true }) == nil)
  }
}

struct RunVideoTests {
  @Test func paintCoversTheMaskedPixelsOfAFrame() throws {
    var made: CVPixelBuffer?
    CVPixelBufferCreate(nil, 8, 4, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, nil, &made)
    let picture = try #require(made)
    CVPixelBufferLockBaseAddress(picture, [])
    for (plane, value) in [(0, UInt8(50)), (1, UInt8(200))] {
      let bytes = CVPixelBufferGetBytesPerRowOfPlane(picture, plane) * CVPixelBufferGetHeightOfPlane(picture, plane)
      CVPixelBufferGetBaseAddressOfPlane(picture, plane)!.initializeMemory(as: UInt8.self, repeating: value, count: bytes)
    }
    CVPixelBufferUnlockBaseAddress(picture, [])

    // A 4×2-point window at (4, 2) on an 8×4-point part of the screen; its left half is hidden.
    let mask = videoMask(
      width: 8, height: 4, shown: [Rect(x: 0, y: 0, width: 8, height: 4)],
      windows: [VideoWindow(bounds: Rect(x: 4, y: 2, width: 4, height: 2), hide: [Rect(x: 0, y: 0, width: 0.5, height: 1)])])
    #expect(RunVideo.paint(mask, on: picture))

    CVPixelBufferLockBaseAddress(picture, .readOnly)
    defer { CVPixelBufferUnlockBaseAddress(picture, .readOnly) }
    let luma = CVPixelBufferGetBaseAddressOfPlane(picture, 0)!.assumingMemoryBound(to: UInt8.self)
    let chroma = CVPixelBufferGetBaseAddressOfPlane(picture, 1)!.assumingMemoryBound(to: UInt8.self)
    let lumaRow = CVPixelBufferGetBytesPerRowOfPlane(picture, 0)
    let chromaRow = CVPixelBufferGetBytesPerRowOfPlane(picture, 1)
    for y in 0..<4 {
      for x in 0..<8 {
        let covered = (4..<6).contains(x) && y >= 2
        #expect(luma[y * lumaRow + x] == (covered ? mask.luma : 50), "luma (\(x), \(y))")
      }
    }
    for y in 0..<2 {
      for x in 0..<8 {
        let covered = (4..<6).contains(x) && y == 1
        let painted = x.isMultiple(of: 2) ? mask.chromaBlue : mask.chromaRed
        #expect(chroma[y * chromaRow + x] == (covered ? painted : 200), "chroma (\(x), \(y))")
      }
    }
  }

  @Test func theFirstPictureStandsInFromTheStartOfTheVideo() throws {
    var made: CVPixelBuffer?
    CVPixelBufferCreate(nil, 4, 4, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, nil, &made)
    let image = try #require(made)
    var format: CMVideoFormatDescription?
    CMVideoFormatDescriptionCreateForImageBuffer(allocator: nil, imageBuffer: image, formatDescriptionOut: &format)
    let late = try CMSampleBuffer(
      imageBuffer: image, formatDescription: try #require(format),
      sampleTiming: CMSampleTimingInfo(
        duration: .invalid, presentationTimeStamp: CMTime(value: 5, timescale: 1), decodeTimeStamp: .invalid))

    let first = try #require(RunVideo.picture(late, at: CMTime(value: 2, timescale: 1)))
    #expect(first.presentationTimeStamp == CMTime(value: 2, timescale: 1))
    #expect(first.imageBuffer === image)
  }

  @Test func paintLeavesOutPicturesOfAnotherFormat() throws {
    var made: CVPixelBuffer?
    CVPixelBufferCreate(nil, 4, 4, kCVPixelFormatType_32BGRA, nil, &made)
    let mask = VideoMask(areas: [PixelArea(x: 0, y: 0, width: 2, height: 2)], luma: 126, chromaBlue: 128, chromaRed: 128)
    #expect(!RunVideo.paint(mask, on: try #require(made)))
  }

  @Test func theVideoRejoinsTheAppThatRelaunchedWhileTheScreenWasListed() async {
    // Listing what can be recorded takes seconds on a busy Mac; meanwhile process 1 quits and 2 starts.
    var relaunched = false
    let slowly: () async -> [pid_t]? = {
      try? await Task.sleep(for: .milliseconds(50))
      relaunched = true
      return [2]
    }
    let rejoined = await RunVideo.rejoining(slowly, pid: { $0 }, running: { relaunched ? [2] : [1] })
    #expect(rejoined == [2])
    // The list can still have the process that quit.
    #expect(await RunVideo.rejoining({ [1, 2] }, pid: { $0 }, running: { [2] }) == [2])
  }

  @Test func theVideoKeepsItsPictureWhileTheAppIsClosedOrNotListedYet() async {
    #expect(await RunVideo.rejoining({ [] }, pid: { $0 }, running: { [] }) == nil)
    #expect(await RunVideo.rejoining({ [1] }, pid: { $0 }, running: { [] }) == nil)
    #expect(await RunVideo.rejoining({ [1] }, pid: { $0 }, running: { [1, 2] }) == nil)
    #expect(await RunVideo.rejoining({ nil }, pid: { $0 }, running: { [1] }) == nil)
  }

  @Test func videoSizeFitsAndIsEven() {
    #expect(RunVideo.videoSize(width: 1280, height: 800) == (1280, 800))
    #expect(RunVideo.videoSize(width: 3024, height: 1964) == (1920, 1246))
    #expect(RunVideo.videoSize(width: 1964, height: 3024) == (1246, 1920))
    #expect(RunVideo.videoSize(width: 1001, height: 601) == (1002, 602))
    #expect(RunVideo.videoSize(width: 0, height: 0) == (2, 2))
  }

  @Test func framingClosesInOnTheWindowsInTheDisplaysProportions() {
    let display = CGRect(x: 1000, y: 0, width: 2000, height: 1000)
    #expect(RunVideo.framing([], on: display) == CGRect(x: 0, y: 0, width: 2000, height: 1000))
    // A 552×352 window with room around becomes 600×400, widened to 2:1.
    #expect(RunVideo.framing([CGRect(x: 1500, y: 300, width: 552, height: 352)], on: display) == CGRect(x: 376, y: 276, width: 800, height: 400))
    // Near the edge the picture moves inside the display instead of showing what's beyond it.
    #expect(RunVideo.framing([CGRect(x: 1000, y: 0, width: 552, height: 352)], on: display) == CGRect(x: 0, y: 0, width: 800, height: 400))
    // Windows on other displays don't count; windows filling the display show all of it.
    #expect(RunVideo.framing([CGRect(x: 0, y: 0, width: 500, height: 500)], on: display) == CGRect(x: 0, y: 0, width: 2000, height: 1000))
    #expect(RunVideo.framing([CGRect(x: 1000, y: 0, width: 2000, height: 980)], on: display) == CGRect(x: 0, y: 0, width: 2000, height: 1000))
  }
}
