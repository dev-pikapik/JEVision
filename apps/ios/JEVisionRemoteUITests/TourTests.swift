import XCTest

/// Walks the app from pairing to a report against a stand-in Mac and keeps a screenshot of
/// every screen. `scripts/ui-tour.sh` starts the stand-in, confirms Face ID and runs it in
/// each language, appearance and text size; the screenshots end up in its output folder.
@MainActor
final class TourTests: XCTestCase {
  private let app = XCUIApplication()

  func testPairsRunsOneRowAndReadsTheReport() throws {
    let environment = ProcessInfo.processInfo.environment
    guard let offer = environment["JEVISION_UI_OFFER"].flatMap(URL.init(string:)) else {
      throw XCTSkip("Run through scripts/ui-tour.sh, which passes a pairing link from the stand-in Mac.")
    }
    continueAfterFailure = false
    if let language = environment["JEVISION_UI_LANGUAGE"] {
      app.launchArguments += ["-AppleLanguages", "(\(language))", "-AppleLocale", language]
    }
    app.launch()
    snap("welcome")
    app.open(offer)

    tap(app.buttons["pair"], "pair")
    // Face ID is confirmed from outside the app, by the script.
    tap(app.buttons["pairDone"], "paired", timeout: 60)

    tap(button(containing: "Test Mac"), "macs")
    tap(button(containing: "Acme Mail"), "projects")
    tap(button(containing: "Greet every person"), "tests")
    XCTAssertTrue(app.buttons["run"].waitForExistence(timeout: 20))
    snap("test")

    tap(app.buttons["rowMenu"], nil)
    snap("rows")
    tap(button(containing: "Boris"), nil)
    XCTAssertTrue((app.buttons["rowMenu"].value as? String)?.contains("Boris") == true)
    snap("row-chosen")

    tap(app.buttons["run"], nil)
    XCTAssertTrue(app.buttons["showReport"].waitForExistence(timeout: 30))
    snap("live-finished")
    tap(app.buttons["showReport"], nil)
    XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", "Boris")).firstMatch.waitForExistence(timeout: 20))
    snap("report")
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

  private func snap(_ name: String) {
    let shot = XCTAttachment(screenshot: app.screenshot())
    shot.name = name
    shot.lifetime = .keepAlways
    add(shot)
  }
}
