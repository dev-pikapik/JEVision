import AppKit
import Foundation
import SwiftUI
import Testing

@testable import JEVision

/// A run of a test that loops long has a report of thousands of steps: it opens, and opening it
/// makes only the steps in view.
@MainActor struct LongReportTests {
  private static let steps = 10_000

  /// A run folder whose report has `steps` steps.
  private static func run() throws -> URL {
    let dir = FileManager.default.temporaryDirectory.appending(path: "jevision-long-report-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let steps = (1...Self.steps).map { seq in
      """
      {"seq": \(seq), "test": "loop.flow.json", "block": "press-\(seq % 7)", "type": "pressKeys",
       "status": "\(seq == Self.steps ? "failed" : "passed")", "durationMs": 2}
      """
    }
    let report = """
      {"format": 1, "test": "Loop", "file": "loop.flow.json", "app": "com.example.Notes",
       "startedAt": "2026-10-19T09:30:00Z", "durationMs": 20000, "verdict": "failed", "flaky": false,
       "attempts": [{"number": 1, "verdict": "failed", "durationMs": 20000, "steps": [\(steps.joined(separator: ","))]}]}
      """
    try report.write(to: dir.appending(path: "report.json"), atomically: true, encoding: .utf8)
    return dir
  }

  @Test func aReportOfTenThousandStepsOpensWithoutHoldingUpTheWindow() async throws {
    let dir = try Self.run()
    defer { try? FileManager.default.removeItem(at: dir) }
    let screen = ScaleUIBenchmarkTests.Offscreen()
    defer { screen.close() }
    let view = RunReportView(dir: dir.path(percentEncoded: false))
      .environment(ProjectStore())
      .environment(RunCenter(defaults: UserDefaults(suiteName: "LongReportTests")!))
    let before = ScaleUIBenchmarkTests.Offscreen.mainCPU()
    screen.show(view)
    // The report is in once its steps scroll: a scroll view replaces the spinner.
    await screen.settleAsync(limit: .seconds(120)) { !screen.views(NSScrollView.self).isEmpty }
    let spent = ScaleUIBenchmarkTests.Offscreen.mainCPU() - before
    #expect(!screen.views(NSScrollView.self).isEmpty, "The report shows")
    // Making every step at once took seconds, then stopped the app; the steps in view take a fraction of that.
    if ProcessInfo.processInfo.environment["JEVISION_SCALE"] == "1" {
      #expect(spent < 3000, "Opening took \(Int(spent)) ms of the main thread")
    }
  }
}
