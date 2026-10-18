import Foundation
import JEVisionCore
import Testing

@testable import JEVision

@MainActor
struct LastRunTests {
  private static let flow = try! loadFlow(
    json: """
      {"format":1,"name":"Sign in","app":"com.example.App","start":"open","blocks":{
        "open":{"type":"openApp","next":"keys"},
        "keys":{"type":"pressKeys","keys":"cmd+s","next":"quit"},
        "quit":{"type":"pressKeys","keys":"cmd+q"}}}
      """)

  private static func step(_ test: String, _ block: String, _ status: StepStatus) -> Step {
    Step(
      seq: 1, test: test, block: block, blockType: "pressKeys", status: status, durationMs: 10, before: nil, after: nil,
      expected: nil, diff: nil, reference: nil, similarity: nil, vision: nil, branch: nil, failure: nil)
  }

  private static func run(_ file: String, at time: String, _ verdict: Verdict, steps: [Step]) -> SavedRun {
    SavedRun(
      dir: "/p/.jevision/runs/\(time)",
      report: RunReport(
        test: file, file: file, app: "com.example.App", startedAt: time, durationMs: 100, verdict: verdict,
        flaky: false, baselinesCreated: [],
        attempts: [
          Attempt(
            number: 1, verdict: verdict, durationMs: 100,
            failure: verdict == .failed ? Failure(code: "run.keys.failed", params: [:]) : nil, steps: steps)
        ]))
  }

  /// Newest first, as the project lists them: a run of a test that ran sign-in, then sign-in's own.
  private static let runs = [
    run(
      "suite.flow.json", at: "2026-10-18T10:00:00Z", .failed,
      steps: [step("sign-in.flow.json", "open", .passed), step("sign-in.flow.json", "keys", .failed)]),
    run("sign-in.flow.json", at: "2026-10-18T09:00:00Z", .passed, steps: [step("sign-in.flow.json", "open", .passed)]),
    run("home.flow.json", at: "2026-10-18T08:00:00Z", .passed, steps: []),
  ]

  @Test func blocksShowTheNewestRunTheyRanInAndTheToolbarTheTestsOwn() {
    let last = LastRun(file: "sign-in.flow.json", flow: Self.flow, runs: Self.runs, pinned: nil)
    #expect(last.own?.report.startedAt == "2026-10-18T09:00:00Z")
    #expect(last.shown?.report.file == "suite.flow.json")
    #expect(last.results.mapValues(\.outcome) == ["open": .passed, "keys": .failed, "quit": .skipped])
    #expect(last.failure(of: "keys")?.failure?.code == "run.keys.failed")
    #expect(last.failure(of: "keys")?.run.report.file == "suite.flow.json")
    #expect(last.failure(of: "open") == nil)
  }

  @Test func aReportPinsItsRun() {
    let pinned = Self.runs[1].id
    let last = LastRun(file: "sign-in.flow.json", flow: Self.flow, runs: Self.runs, pinned: pinned)
    #expect(last.shown?.id == pinned)
    #expect(last.results.mapValues(\.outcome) == ["open": .passed])
  }

  @Test func aTestNeverRunHasNothing() {
    let last = LastRun(file: "new.flow.json", flow: Self.flow, runs: Self.runs, pinned: nil)
    #expect(last.own == nil && last.shown == nil && last.results.isEmpty)
  }

  @Test func tagsAreTrimmedWithoutEmptyOnesOrRepeats() {
    #expect(TestFields.tags(" smoke, sign-in,, smoke ,") == ["smoke", "sign-in"])
    #expect(TestFields.tags("  ").isEmpty)
  }
}
