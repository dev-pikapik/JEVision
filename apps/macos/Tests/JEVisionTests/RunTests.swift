import Foundation
import JEVisionCore
import Testing

@testable import JEVision

struct RunStateTests {
  private func started(_ block: String, type: String = "click", in test: String = "a.flow.json") -> RunEvent {
    .stepStarted(attempt: 1, seq: 0, test: test, block: block, blockType: type, variables: [:])
  }

  private func finished(_ block: String, _ status: StepStatus, in test: String = "a.flow.json") -> RunEvent {
    .stepFinished(
      attempt: 1,
      step: Step(
        seq: 0, test: test, block: block, blockType: "click", status: status, durationMs: 0, before: nil,
        after: nil, expected: nil, diff: nil, reference: nil, similarity: nil, branch: nil, failure: nil))
  }

  private func run(_ events: [RunEvent]) -> RunState {
    var state = RunState()
    for event in [.runStarted(test: "Sign in", file: "a.flow.json", dir: "/r", maxAttempts: 2), .attemptStarted(attempt: 1)] + events {
      state.apply(event)
    }
    return state
  }

  @Test func followsTheBlocksOfTheTest() {
    let state = run([started("open"), finished("open", .passed), started("check")])
    #expect(state.name == "Sign in")
    #expect(state.maxAttempts == 2)
    #expect(state.current == "check")
    #expect(state.marks == ["open": .passed])
    #expect(state.done == 1)
    #expect(state.phase == .running)
  }

  @Test func marksFailuresAndNewReferences() {
    let state = run([started("a"), finished("a", .baselineCreated), started("b"), finished("b", .failed)])
    #expect(state.marks == ["a": .passed, "b": .failed])
    #expect(state.current == nil)
  }

  @Test func showsWherePausesHappen() {
    var state = run([started("a"), finished("a", .passed), .paused(block: "b", reason: .breakpoint)])
    #expect(state.phase == .paused(.breakpoint))
    #expect(state.isPaused)
    #expect(state.current == "b")
    state.apply(.resumed)
    #expect(state.phase == .running)
    state.apply(.paused(block: "b", reason: .userInput))
    #expect(state.phase == .paused(.userInput))
  }

  @Test func blocksOfAnotherTestDontMarkThisOne() {
    var state = run([started("login", type: "runTest"), started("open", in: "shared/sign-in.flow.json")])
    #expect(state.current == nil)
    #expect(state.currentType == "click")
    state.apply(.paused(block: "type", reason: .step))
    #expect(state.current == nil)
    state.apply(finished("open", .passed, in: "shared/sign-in.flow.json"))
    #expect(state.marks.isEmpty)
    #expect(state.current == "login")
    state.apply(finished("login", .passed))
    #expect(state.marks == ["login": .passed])
  }

  @Test func aRetryStartsClean() {
    var state = run([started("a"), finished("a", .failed)])
    let failure = Failure(code: "run.target.notFound", params: ["block": "a", "image": "ok.png"])
    state.apply(.attemptFinished(attempt: 1, verdict: .failed, failure: failure))
    state.apply(.attemptStarted(attempt: 2))
    #expect(state.attempt == 2)
    #expect(state.marks.isEmpty)
    #expect(state.failure == failure)
    state.apply(.runFinished(verdict: .passed, flaky: true, report: "/r/report.json"))
    #expect(state.phase == .finished(.passed))
    #expect(state.flaky)
  }
}

@MainActor
struct RunTextTests {
  @Test func describesFailures() {
    let missing = Failure(code: "run.secret.missing", params: ["name": "PASSWORD"])
    #expect(RunText.failure(missing) == "The secret PASSWORD isn’t set. Set it in Test details.")
    let settle = Failure(code: "run.waitFor.timeout", params: ["block": "w", "target": "screenStable", "seconds": "10"])
    #expect(RunText.failure(settle) == "Block “w”: the screen didn’t settle within 10 s.")
  }

  @Test func pointsToTheMissingPermission() {
    let denied = Failure(code: "run.hands.permissionDenied", params: ["block": "c", "permission": "screenRecording"])
    #expect(RunText.permission(of: denied) == .screenRecording)
    #expect(RunText.failure(denied).contains(Permission.screenRecording.title))
    #expect(RunText.permission(of: Failure(code: "run.hands.failed", params: [:])) == nil)
  }
}

struct GitIgnoreTests {
  private func folder() throws -> URL {
    let url = FileManager.default.temporaryDirectory.appending(path: "jevision-git-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: url.appending(path: "tests"), withIntermediateDirectories: true)
    return url
  }

  @Test func offersOnlyInsideARepository() throws {
    let repo = try folder()
    defer { try? FileManager.default.removeItem(at: repo) }
    let project = repo.appending(path: "tests")
    #expect(!GitIgnore.needsEntry(in: project))
    try FileManager.default.createDirectory(at: repo.appending(path: ".git/info"), withIntermediateDirectories: true)
    #expect(GitIgnore.needsEntry(in: project))

    try "node_modules\n.jevision\n".write(to: repo.appending(path: ".gitignore"), atomically: true, encoding: .utf8)
    #expect(!GitIgnore.needsEntry(in: project))
    // An anchored line in a parent folder is about that folder, not the project.
    try "/.jevision/\n".write(to: repo.appending(path: ".gitignore"), atomically: true, encoding: .utf8)
    #expect(GitIgnore.needsEntry(in: project))
  }

  @Test func addsTheLine() throws {
    let project = try folder()
    defer { try? FileManager.default.removeItem(at: project) }
    try FileManager.default.createDirectory(at: project.appending(path: ".git"), withIntermediateDirectories: true)
    try "build".write(to: project.appending(path: ".gitignore"), atomically: true, encoding: .utf8)
    #expect(GitIgnore.needsEntry(in: project))
    try GitIgnore.add(to: project)
    #expect(try String(contentsOf: project.appending(path: ".gitignore"), encoding: .utf8) == "build\n.jevision/\n")
    #expect(!GitIgnore.needsEntry(in: project))
  }
}

@MainActor
struct BreakpointTests {
  @Test func surviveARestartAndForgetDeletedBlocks() throws {
    let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: folder) }
    let url = folder.appending(path: "t.flow.json")
    try """
      {"format": 1, "name": "T", "app": "com.apple.TextEdit", "start": "a", "blocks": {
        "a": {"type": "pressKeys", "keys": "a", "next": "b"},
        "b": {"type": "pressKeys", "keys": "b"}}}
      """.write(to: url, atomically: true, encoding: .utf8)
    let suite = "jevision-tests-\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    defer { defaults.removePersistentDomain(forName: suite) }
    let document = FlowDocument(url: url)

    RunCenter(defaults: defaults).toggleBreakpoint("b", in: document)
    let reopened = RunCenter(defaults: defaults)
    #expect(reopened.hasBreakpoint("b", in: document))
    #expect(!reopened.hasBreakpoint("a", in: document))

    document.delete("b")
    reopened.toggleBreakpoint("a", in: document)
    #expect(RunCenter(defaults: defaults).breakpoints == [url: ["a"]])
  }
}
