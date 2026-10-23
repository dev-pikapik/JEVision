import Foundation
import JEVisionCore
import Testing

@testable import JEVision

/// The index of the runs finds the same runs as looking through them all does.
@MainActor
struct RunIndexTests {
  private static func step(_ test: String, _ status: StepStatus) -> Step {
    Step(
      seq: 1, test: test, block: "open", blockType: "openApp", status: status, durationMs: 10, before: nil, after: nil,
      expected: nil, diff: nil, reference: nil, similarity: nil, vision: nil, branch: nil, failure: nil)
  }

  private static func attempt(_ number: UInt32, _ verdict: Verdict, _ steps: [Step]) -> Attempt {
    Attempt(
      number: number, verdict: verdict, durationMs: 100,
      failure: verdict == .failed ? Failure(code: "run.keys.failed", params: [:]) : nil, steps: steps)
  }

  private static func run(_ file: String, at hour: Int, _ verdict: Verdict, ci: Bool = false, _ attempts: [Attempt])
    -> SavedRun
  {
    let time = String(format: "2026-10-18T%02d:00:00Z", hour)
    return SavedRun(
      dir: "/p/.jevision/runs/\(time)-\(file)",
      report: RunReport(
        test: file, file: file, app: "com.example.App", startedAt: time, durationMs: 100, verdict: verdict,
        flaky: attempts.count > 1 && verdict == .passed, baselinesCreated: [],
        ci: ci ? CiOrigin(system: "github", repository: nil, branch: "main", commit: nil, pullRequest: nil, runUrl: nil) : nil,
        attempts: attempts))
  }

  /// Newest first: a CI run, a suite that failed inside sign-in, a suite that went into cart only on
  /// a try that failed before a retry passed, a test with no steps, and the tests' own runs.
  private static let runs = [
    run("suite.flow.json", at: 12, .failed, ci: true, [attempt(1, .failed, [step("home.flow.json", .failed)])]),
    run("suite.flow.json", at: 11, .failed, [attempt(1, .failed, [step("sign-in.flow.json", .failed)])]),
    run(
      "retry.flow.json", at: 10, .passed,
      [attempt(1, .failed, [step("cart.flow.json", .failed)]), attempt(2, .passed, [step("retry.flow.json", .passed)])]),
    run("empty.flow.json", at: 9, .passed, []),
    run("sign-in.flow.json", at: 8, .passed, [attempt(1, .passed, [step("sign-in.flow.json", .passed)])]),
    run("cart.flow.json", at: 7, .failed, [attempt(1, .failed, [step("cart.flow.json", .failed)])]),
    run("home.flow.json", at: 6, .passed, [attempt(1, .passed, [step("home.flow.json", .passed)])]),
  ]

  @Test func findsTheRunsLookingThroughThemFinds() {
    let index = RunIndex(Self.runs)
    let files = [
      "suite.flow.json", "sign-in.flow.json", "retry.flow.json", "cart.flow.json", "home.flow.json", "empty.flow.json",
      "never.flow.json",
    ]
    for file in files {
      #expect(index.own(file)?.id == LastRun.own(file, in: Self.runs)?.id, "own run of \(file)")
      #expect(index.ran(file)?.id == LastRun.ran(file, in: Self.runs)?.id, "last run in which \(file) ran")
    }
    #expect(index.ran("sign-in.flow.json")?.id == Self.runs[1].id)
    #expect(index.ran("home.flow.json")?.id == Self.runs[6].id)
    for run in Self.runs { #expect(index.run(run.id)?.id == run.id) }
    #expect(index.run("/p/.jevision/runs/gone") == nil)
  }

  @Test func showsWhatLookingThroughTheRunsShows() throws {
    let flow = try loadFlow(
      json: #"{"format":1,"name":"Sign in","app":"com.example.App","start":"open","blocks":{"open":{"type":"openApp"}}}"#)
    let index = RunIndex(Self.runs)
    for pinned in [nil, Self.runs[0].id, Self.runs[4].id] {
      let looked = LastRun(file: "sign-in.flow.json", flow: flow, runs: Self.runs, pinned: pinned)
      let indexed = LastRun(file: "sign-in.flow.json", flow: flow, index: index, pinned: pinned)
      #expect(indexed.own?.id == looked.own?.id)
      #expect(indexed.shown?.id == looked.shown?.id)
      #expect(indexed.results == looked.results)
      #expect(indexed.taken == looked.taken)
    }
  }
}
