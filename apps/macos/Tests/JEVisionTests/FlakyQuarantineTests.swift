import Foundation
import JEVisionCore
import Testing

@testable import JEVision

struct FlakyQuarantineTests {
  // MARK: Quarantine until a day

  @Test func aDayRoundTripsThroughThePickerDate() throws {
    let date = try #require(QuarantineDay.date("2026-11-03"))
    #expect(QuarantineDay.string(date) == "2026-11-03")
    #expect(Calendar.current.component(.day, from: date) == 3)
    #expect(QuarantineDay.date("soon") == nil)
    #expect(QuarantineDay.title("soon") == "soon")
  }

  @Test func aNewQuarantineLastsTwoWeeksUnlessItHasNoEnd() throws {
    let draft = QuarantineDraft()
    #expect(draft.lastDay == defaultQuarantineEnd(today: localToday()))
    let today = try #require(QuarantineDay.date(localToday()))
    let days = Calendar.current.dateComponents([.day], from: today, to: draft.until).day
    #expect(days == 14)
    var open = draft
    open.returns = false
    #expect(open.lastDay == nil)
  }

  @Test func changingAQuarantineStartsFromIt() {
    let draft = QuarantineDraft(Quarantined(test: "t.flow.json", reason: "Bug 7", until: "2026-12-01"))
    #expect(draft.reason == "Bug 7")
    #expect(draft.lastDay == "2026-12-01")
    let open = QuarantineDraft(Quarantined(test: "t.flow.json", reason: nil))
    #expect(!open.returns)
  }

  @Test func marksTellUntilWhenAndWhetherItEnded() {
    let end = QuarantineDay.title("2026-11-03")
    let timed = Quarantined(test: "t.flow.json", reason: "Bug 7", until: "2026-11-03")
    #expect(QuarantineText.describe(timed, today: "2026-11-03") == "In quarantine until \(end): Bug 7")
    #expect(QuarantineText.describe(timed, today: "2026-11-04") == "Quarantine ended \(end): Bug 7")
    let bare = Quarantined(test: "t.flow.json", reason: nil, until: "2026-11-03")
    #expect(QuarantineText.describe(bare, today: "2026-10-20") == "In quarantine until \(end)")
    #expect(QuarantineText.describe(Quarantined(test: "t.flow.json", reason: "Bug 7")) == "In quarantine: Bug 7")
    #expect(QuarantineText.describe(Quarantined(test: "t.flow.json", reason: nil)) == "In quarantine")
  }

  @Test func theSuggestionNamesTheUnreliableRuns() {
    let evidence = FlakyEvidence(passed: 3, passedOnRetry: 1, failed: 2, environment: nil, since: "2026-10-01T09:00:00Z")
    let events = [
      FlakyEvent(startedAt: "2026-10-15T09:00:00Z", kind: .passedOnRetry),
      FlakyEvent(startedAt: "2026-10-17T09:00:00Z", kind: .failedAfterPass),
    ]
    let suggestion = QuarantineSuggestion(evidence: evidence, events: events)
    #expect(QuarantineText.suggestion(suggestion) == "Unreliable 2 times in the last 7 days")
    #expect(QuarantineText.event(events[0]).hasSuffix("Passed only on a retry"))
    #expect(QuarantineText.event(events[1]).hasSuffix("Failed after passing, nothing changed"))
  }

  @Test func theEvidenceCountsOnlyWhatHappened() {
    func text(_ passed: UInt32, _ retry: UInt32, _ failed: UInt32) -> String {
      RunHistory.flakyEvidence(
        FlakyEvidence(passed: passed, passedOnRetry: retry, failed: failed, environment: nil, since: "2026-10-01T09:00:00Z"))
    }
    // Passes after a retry are no failed run: "Failed: 0" would contradict the pass rate beside it.
    #expect(!text(1, 1, 0).contains("Failed"))
    #expect(text(1, 1, 0).hasSuffix("Passed: 1 · Passed on retry: 1"))
    #expect(text(0, 2, 0).hasSuffix("Passed on retry: 2"))
    #expect(text(2, 0, 1).hasSuffix("Passed: 2 · Failed: 1"))
  }

  @Test func theEvidenceNamesTheDataRow() {
    let row = DataRow(number: 2, label: "anna", hash: "0123456789abcdef")
    let evidence = FlakyEvidence(
      passed: 2, passedOnRetry: 0, failed: 1, environment: nil, since: "2026-10-01T09:00:00Z", row: row)
    #expect(RunHistory.flakyEvidence(evidence).contains("Data row: Row 2: anna."))
    let plain = FlakyEvidence(passed: 2, passedOnRetry: 0, failed: 1, environment: nil, since: "2026-10-01T09:00:00Z")
    #expect(!RunHistory.flakyEvidence(plain).contains("Data row"))
  }

  // MARK: Choosing tests by their cards

  private static func test(
    _ id: String, priority: Priority? = nil, type: TestType? = nil, author: String? = nil, flaky: Bool = false
  ) -> ProjectItem {
    ProjectItem(
      id: id, name: id, url: URL(filePath: "/project/" + id), kind: .test, children: [], testCount: 1, modified: nil,
      card: JEVisionCore.TestCard(testType: type, priority: priority, status: nil, author: author, flaky: flaky))
  }

  private static func folder(_ id: String, _ children: [ProjectItem]) -> ProjectItem {
    ProjectItem(
      id: id, name: id, url: URL(filePath: "/project/" + id), kind: .folder, children: children,
      testCount: children.reduce(0) { $0 + $1.testCount }, modified: nil)
  }

  private static let items = [
    folder(
      "Smoke",
      [
        test("Smoke/login", priority: .high, type: .smoke, author: "Anna Lee"),
        test("Smoke/logout", priority: .low, type: .smoke),
      ]),
    folder("Empty", [test("Empty/x", priority: .low)]),
    test("checkout", priority: .high, author: "ben", flaky: true),
  ]

  @Test func anEmptyFilterChoosesEveryTest() {
    let all = Self.items.chosen(by: TestFilter(), flaky: [])
    #expect(all.map(\.id) == ["Smoke/login", "Smoke/logout", "Empty/x", "checkout"])
  }

  @Test func theFilterChoosesByCardFields() {
    let high = Self.items.chosen(by: TestFilter(priority: .high), flaky: [])
    #expect(high.map(\.id) == ["Smoke/login", "checkout"])
    let smokeByAnna = Self.items.chosen(by: TestFilter(testType: .smoke, author: " anna lee "), flaky: [])
    #expect(smokeByAnna.map(\.id) == ["Smoke/login"])
    #expect(Self.items.chosen(by: TestFilter(priority: .critical), flaky: []).isEmpty)
  }

  @Test func flakyCountsByTheMarkOrByTheRuns() {
    #expect(Self.items.chosen(by: TestFilter(flaky: true), flaky: []).map(\.id) == ["checkout"])
    let byRuns = Self.items.chosen(by: TestFilter(flaky: true), flaky: ["Smoke/logout"])
    #expect(byRuns.map(\.id) == ["Smoke/logout", "checkout"])
  }

  @Test func theSidebarKeepsOnlyFoldersThatHoldAChosenTest() {
    let kept = Self.items.keeping(["Smoke/login", "checkout"])
    #expect(kept.map(\.id) == ["Smoke", "checkout"])
    #expect(kept[0].children.map(\.id) == ["Smoke/login"])
    #expect(kept[0].testCount == 1)
    #expect(Self.items.keeping([]).isEmpty)
  }
}
