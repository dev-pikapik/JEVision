import Foundation
import JEVisionCore
import JEVisionHands
import Testing

@testable import JEVision

/// Runs a `RunCenter` starts in these tests: they touch nothing and end when the test says.
@MainActor
private final class FakeRuns {
  struct Started {
    let name: String
    let run: TestRun
    let listener: any RunListener
  }

  private(set) var started: [Started] = []
  private var ended: Set<String> = []
  /// Tests refused once, as if something they need were missing.
  var refusing: Set<String> = []

  lazy var center = RunCenter(
    defaults: UserDefaults(suiteName: "RunAllTests")!, starts: { [unowned self] request, run, listener in
      let name = request.document.url.deletingPathExtension().deletingPathExtension().lastPathComponent
      if self.refusing.remove(name) != nil { return false }
      self.started.append(Started(name: name, run: run, listener: listener))
      return true
    }, makeRun: { WatchedRun() })

  /// The tests started and not ended yet, in the order they started.
  var going: [String] { started.map(\.name).filter { !ended.contains($0) } }

  func run(_ name: String) throws -> TestRun {
    try #require(started.first { $0.name == name }, "\(name) started").run
  }

  /// What the HUD's buttons asked of the run of `name`.
  func asked(_ name: String) throws -> [String] {
    try #require(run(name) as? WatchedRun).asked
  }

  /// Ends the run of `name` as the core would, and lets the center hear it.
  func end(_ name: String, _ verdict: Verdict) async throws {
    let started = try #require(started.first { $0.name == name }, "\(name) started")
    ended.insert(name)
    started.listener.finished(
      outcome: .finished(
        dir: "/p/.jevision/runs/\(name)", html: nil,
        report: RunReport(
          test: name, file: "smoke/\(name).flow.json", app: "com.apple.TextEdit", startedAt: "2026-10-20T10:00:00Z",
          durationMs: 1, verdict: verdict, flaky: false, baselinesCreated: [], attempts: [])))
    // The center hears it on the main queue, after what is there already.
    await withCheckedContinuation { done in DispatchQueue.main.async { done.resume() } }
  }
}

/// A run that remembers what it was asked to do.
private final class WatchedRun: TestRun, @unchecked Sendable {
  private(set) var asked: [String] = []

  override func pause() {
    asked.append("pause")
    super.pause()
  }

  override func resume() {
    asked.append("resume")
    super.resume()
  }

  override func step() {
    asked.append("step")
    super.step()
  }
}

@MainActor
struct RunAllTests {
  /// A project whose folder "smoke" runs as `mode`, with a test for each name; the tests in
  /// `screen` type on the Mac, the others only wait.
  private static func project(_ names: [String], screen: Set<String> = [], mode: RunMode) throws -> (URL, [URL]) {
    let project = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    let folder = project.appending(path: "smoke")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let tests = try names.map { name in
      let block = screen.contains(name) ? #"{"type": "pressKeys", "keys": "a"}"# : #"{"type": "pause", "duration": "1s"}"#
      let url = folder.appending(path: "\(name).flow.json")
      try """
        {"format": 1, "name": "\(name)", "app": "com.apple.TextEdit", "start": "a", "blocks": {"a": \(block)}}
        """.write(to: url, atomically: true, encoding: .utf8)
      return url
    }
    _ = try setFolderRunMode(project: project.path, folder: "smoke", mode: mode)
    return (project, tests)
  }

  private static func counts(_ result: RunCenter.BatchResult?) -> [Int]? {
    result.map { [$0.total, $0.passed, $0.failed, $0.stopped ? 1 : 0] }
  }

  @Test func sideBySideStartsUpToTheLimitAndTheNextTestWhenOneEnds() async throws {
    let (project, tests) = try Self.project(["a", "b", "c"], mode: .parallel(limit: 2))
    defer { try? FileManager.default.removeItem(at: project) }
    let fake = FakeRuns()
    fake.center.runAll(tests, in: project, title: "Smoke", folder: "smoke")
    #expect(fake.going == ["a", "b"], "two start at once")
    try await fake.end("a", .passed)
    #expect(fake.going == ["b", "c"], "the third starts when one ends")
    try await fake.end("c", .failed)
    #expect(fake.center.batch != nil, "the batch waits for the test still running")
    try await fake.end("b", .passed)
    #expect(fake.center.batch == nil)
    #expect(Self.counts(fake.center.batchResult) == [3, 2, 1, 0])
  }

  @Test func oneAfterAnotherRunsOneTestAtATime() async throws {
    let (project, tests) = try Self.project(["a", "b", "c"], mode: .sequential)
    defer { try? FileManager.default.removeItem(at: project) }
    let fake = FakeRuns()
    fake.center.runAll(tests, in: project, title: "Smoke", folder: "smoke")
    #expect(fake.going == ["a"])
    try await fake.end("a", .failed)
    #expect(fake.going == ["b"])
    try await fake.end("b", .passed)
    #expect(fake.going == ["c"])
    try await fake.end("c", .passed)
    #expect(fake.center.batch == nil)
    #expect(Self.counts(fake.center.batchResult) == [3, 2, 1, 0])
  }

  @Test func testsThatUseTheScreenTakeTurnsWhileOthersGoAlongside() async throws {
    let (project, tests) = try Self.project(["s1", "s2", "n"], screen: ["s1", "s2"], mode: .parallel(limit: 2))
    defer { try? FileManager.default.removeItem(at: project) }
    let fake = FakeRuns()
    fake.center.runAll(tests, in: project, title: "Smoke", folder: "smoke")
    #expect(fake.going == ["s1", "n"], "the other test on the screen waits; one that leaves it alone goes")
    #expect(fake.center.running?.document.url == tests[0], "the HUD shows the test on the screen")
    try await fake.end("s1", .passed)
    #expect(fake.going == ["n", "s2"])
    #expect(fake.center.running?.document.url == tests[1], "the HUD moves to the next test on the screen")
    try await fake.end("s2", .passed)
    #expect(fake.center.running?.document.url == tests[2])
    try await fake.end("n", .passed)
    #expect(Self.counts(fake.center.batchResult) == [3, 3, 0, 0])
  }

  @Test func stopCancelsEveryRunningTestAndStartsNoMore() async throws {
    let (project, tests) = try Self.project(["a", "b", "c"], mode: .parallel(limit: 2))
    defer { try? FileManager.default.removeItem(at: project) }
    let fake = FakeRuns()
    fake.center.runAll(tests, in: project, title: "Smoke", folder: "smoke")
    #expect(fake.going == ["a", "b"])
    fake.center.stop()
    #expect(try fake.run("a").isCancelled() && fake.run("b").isCancelled())
    try await fake.end("a", .cancelled)
    #expect(fake.center.batch != nil, "the batch ends once the other running test does")
    try await fake.end("b", .cancelled)
    #expect(fake.started.count == 2, "the waiting test doesn't start")
    #expect(fake.center.batch == nil)
    #expect(Self.counts(fake.center.batchResult) == [3, 0, 0, 1])
  }

  @Test func theSheetOfATestThatCantStartHoldsBackOnlyTheTestsAfterIt() async throws {
    let (project, tests) = try Self.project(["s", "n", "m"], screen: ["s"], mode: .parallel(limit: 2))
    defer { try? FileManager.default.removeItem(at: project) }
    let fake = FakeRuns()
    fake.refusing = ["n"]
    fake.center.runAll(tests, in: project, title: "Smoke", folder: "smoke")
    let sheet = try #require(fake.center.blocked, "the sheet asks for what n needs")
    #expect(sheet.document.url == tests[1])
    #expect(fake.going == ["s"], "the running test goes on; no other starts while the sheet is open")
    fake.center.confirm()
    #expect(fake.center.blocked == nil, "the sheet closes once its test starts, though another one runs")
    #expect(fake.going == ["s", "n"])
    try await fake.end("s", .passed)
    #expect(fake.going == ["n", "m"])
    try await fake.end("n", .passed)
    try await fake.end("m", .passed)
    #expect(Self.counts(fake.center.batchResult) == [3, 3, 0, 0])
  }

  @Test func cancellingTheSheetStopsTheBatchOnceTheRunningTestsEnd() async throws {
    let (project, tests) = try Self.project(["s", "n", "m"], screen: ["s"], mode: .parallel(limit: 2))
    defer { try? FileManager.default.removeItem(at: project) }
    let fake = FakeRuns()
    fake.refusing = ["n"]
    fake.center.runAll(tests, in: project, title: "Smoke", folder: "smoke")
    try #require(fake.center.blocked != nil)
    fake.center.blocked = nil
    #expect(try fake.run("s").isCancelled(), "the running test is stopped as by Stop")
    #expect(fake.center.batch != nil, "the batch waits for it")
    try await fake.end("s", .cancelled)
    #expect(fake.started.map(\.name) == ["s"])
    #expect(fake.center.batch == nil)
    #expect(Self.counts(fake.center.batchResult) == [3, 0, 0, 1])
  }

  @Test func aSheetWaitingForAPersonKeepsOtherRunsFromStarting() async throws {
    let (project, tests) = try Self.project(["a", "b"], mode: .sequential)
    defer { try? FileManager.default.removeItem(at: project) }
    let fake = FakeRuns()
    fake.refusing = ["a"]
    fake.center.run(FlowDocument(url: tests[0], project: project), in: project)
    try #require(fake.center.blocked != nil, "the sheet asks for what a needs")
    fake.center.run(FlowDocument(url: tests[1], project: project), in: project)
    #expect(fake.started.isEmpty, "a test doesn't start beside a sheet still open")
    #expect(fake.center.runRemotely("smoke/b.flow.json", in: project, requestedBy: "iPhone") == .RunInProgress)
    fake.center.runAll(tests, in: project, title: "Smoke", folder: "smoke")
    #expect(fake.started.isEmpty && fake.center.batch == nil, "nor does a batch")
    #expect(fake.center.isBusy, "the Run buttons are off while the sheet is open")
    fake.center.blocked = nil
    #expect(!fake.center.isBusy)
  }

  @Test func theHUDCountsTheTestsDoneAndRunningWhileSeveralRun() async throws {
    let (project, tests) = try Self.project(["a", "b", "c"], mode: .parallel(limit: 2))
    defer { try? FileManager.default.removeItem(at: project) }
    let fake = FakeRuns()
    fake.center.runAll(tests, in: project, title: "Smoke", folder: "smoke")
    #expect(fake.center.batch?.progress == "0 of 3 done · 2 running")
    try await fake.end("a", .passed)
    #expect(fake.center.batch?.progress == "1 of 3 done · 2 running")
    try await fake.end("b", .failed)
    #expect(fake.center.batch?.progress == "Test 3 of 3", "one test running counts as one after another")
    try await fake.end("c", .passed)
  }

  @Test func pauseResumeAndStepActOnlyOnTheRunTheHUDShows() async throws {
    let (project, tests) = try Self.project(["n", "s"], screen: ["s"], mode: .parallel(limit: 2))
    defer { try? FileManager.default.removeItem(at: project) }
    let fake = FakeRuns()
    // The window stays where it is: a test has no app to hide.
    UserDefaults(suiteName: "RunAllTests")!.set(false, forKey: RunInputSettings.collapsesKey)
    fake.center.runAll(tests, in: project, title: "Smoke", folder: "smoke")
    try #require(fake.going.count == 2)
    #expect(fake.center.running?.document.url == tests[1], "the HUD shows the test on the screen")
    fake.center.pause()
    fake.center.resume()
    fake.center.step()
    #expect(try fake.asked("s") == ["pause", "resume", "step"])
    #expect(try fake.asked("n").isEmpty, "the test beside it goes on")
    fake.center.stop()
    try await fake.end("n", .cancelled)
    try await fake.end("s", .cancelled)
  }

  @Test func escEscCancelsEveryRunAtOnceAndStopsTheBatch() async throws {
    let (project, tests) = try Self.project(["a", "b", "c"], mode: .parallel(limit: 2))
    defer { try? FileManager.default.removeItem(at: project) }
    let fake = FakeRuns()
    fake.center.runAll(tests, in: project, title: "Smoke", folder: "smoke")
    #expect(fake.going == ["a", "b"])
    let center = fake.center
    // The input watcher calls it on its own thread, while the main thread may be busy.
    await Task.detached { center.emergencyStop() }.value
    #expect(try fake.run("a").isCancelled() && fake.run("b").isCancelled(), "cancelled before the main thread hears of it")
    try await fake.end("a", .cancelled)
    try await fake.end("b", .cancelled)
    #expect(fake.started.count == 2, "the waiting test doesn't start")
    #expect(fake.center.batch == nil)
    #expect(Self.counts(fake.center.batchResult) == [3, 0, 0, 1])
  }
}
