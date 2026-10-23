import XCTest

/// Takes the App Store screenshots in the built-in example, which needs no Mac and no network.
/// `scripts/screenshots.sh` runs it on each iPhone size in each language and sorts the
/// pictures; each one is named after its place in the App Store, then the screen.
@MainActor
final class ScreenshotTests: XCTestCase {
  private let app = XCUIApplication()

  /// What the example's Mac and tests are called in each language the App Store page has.
  private static let labels: [String: (mac: String, people: String, signIn: String, screen: String)] = [
    "en": ("Example Mac", "Greet every person", "Sign in with valid credentials", "Screen After"),
    "ru": ("Mac для примера", "Приветствие для каждого", "Вход с верными данными", "Экран после"),
  ]

  func testTakesTheAppStoreScreenshots() throws {
    let environment = ProcessInfo.processInfo.environment
    guard let language = environment["JEVISION_SCREENSHOTS_LANGUAGE"], let labels = Self.labels[language] else {
      throw XCTSkip("Run through scripts/screenshots.sh, which names the language.")
    }
    continueAfterFailure = false
    app.launchArguments += ["-AppleLanguages", "(\(language))", "-AppleLocale", language]
    app.launch()

    tap(app.buttons["example"], "9-welcome")
    tap(button(containing: labels.mac), "8-macs")
    tap(button(containing: "Acme Mail"), nil)
    tap(button(containing: labels.people), "7-tests")
    XCTAssertTrue(app.buttons["run"].waitForExistence(timeout: 20))
    snap("1-test")

    let note = app.buttons["note-greeting"]
    XCTAssertTrue(note.waitForExistence(timeout: 20))
    for _ in 0..<8 where !note.isHittable { app.swipeUp() }
    tap(note, nil)
    XCTAssertTrue(app.buttons["noteDone"].waitForExistence(timeout: 10))
    snap("6-note")
    app.buttons["noteDone"].tap()

    for _ in 0..<8 where !app.buttons["run"].isHittable { app.swipeDown() }
    tap(app.buttons["run"], nil)
    XCTAssertTrue(app.buttons["showReport"].waitForExistence(timeout: 30))
    // The header and the flowchart turn to the verdict with an animation.
    sleep(1)
    snap("2-run")
    app.buttons["showReport"].tap()
    XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", "Chen")).firstMatch.waitForExistence(timeout: 20))
    snap("3-report")

    // Back to the project, then into the test that fails.
    for _ in 0..<3 { app.navigationBars.buttons.element(boundBy: 0).tap() }
    tap(button(containing: "login"), nil)
    tap(button(containing: labels.signIn), nil)
    tap(app.buttons["run"], nil)
    tap(app.buttons["showReport"], nil, timeout: 30)
    // The failed check, opened to the screen the Mac saw and the picture it expected.
    tap(button(containing: "messages"), "4-failed")
    XCTAssertTrue(app.images[labels.screen].waitForExistence(timeout: 20))
    app.swipeUp()
    snap("5-pictures")
  }

  private func button(containing text: String) -> XCUIElement {
    app.buttons.matching(NSPredicate(format: "label CONTAINS %@", text)).firstMatch
  }

  /// Waits for an element, keeps a screenshot of the screen it is on when `screen` is named,
  /// and taps it.
  private func tap(_ element: XCUIElement, _ screen: String?, timeout: TimeInterval = 20) {
    XCTAssertTrue(element.waitForExistence(timeout: timeout), "\(element) didn't show up")
    if let screen { snap(screen) }
    element.tap()
  }

  /// The whole screen, status bar included, at the device's own resolution.
  private func snap(_ name: String) {
    let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
    shot.name = name
    shot.lifetime = .keepAlways
    add(shot)
  }
}
