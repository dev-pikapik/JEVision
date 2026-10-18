import CoreGraphics
import Foundation
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

struct SyntheticInputTests {
  @Test func tellsTheTestsInputFromAPersons() throws {
    let own = try #require(CGEvent(mouseEventSource: SyntheticInput.source, mouseType: .mouseMoved, mouseCursorPosition: .zero, mouseButton: .left))
    #expect(SyntheticInput.isSynthetic(own))
    let person = try #require(
      CGEvent(keyboardEventSource: CGEventSource(stateID: .hidSystemState), virtualKey: 53, keyDown: true))
    #expect(!SyntheticInput.isSynthetic(person))
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
