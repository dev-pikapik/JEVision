import Foundation
import JEVisionCore
import Testing

@testable import JEVision

struct RunStateTests {
  private func started(
    _ block: String, type: String = "click", in test: String = "a.flow.json", timeoutMs: UInt64? = nil
  ) -> RunEvent {
    .stepStarted(attempt: 1, seq: 0, test: test, block: block, blockType: type, variables: [:], timeoutMs: timeoutMs)
  }

  private func finished(_ block: String, _ status: StepStatus, in test: String = "a.flow.json") -> RunEvent {
    .stepFinished(
      attempt: 1,
      step: Step(
        seq: 0, test: test, block: block, blockType: "click", status: status, durationMs: 0, before: nil,
        after: nil, expected: nil, diff: nil, reference: nil, similarity: nil, vision: nil, branch: nil, failure: nil))
  }

  private func run(_ events: [RunEvent]) -> RunState {
    var state = RunState()
    for event in [.runStarted(test: "Sign in", file: "a.flow.json", dir: "/r", maxAttempts: 2), .attemptStarted(attempt: 1)] + events {
      state.apply(event)
    }
    return state
  }

  private static func choice(
    _ branch: String, of branches: UInt32?, saw condition: String? = nil, in test: String = "a.flow.json"
  ) -> Step {
    Step(
      seq: 0, test: test, block: "if", blockType: "if", status: .passed, durationMs: 0, before: nil, after: nil,
      expected: nil, diff: nil, reference: nil, similarity: nil, vision: nil, branch: branch, failure: nil,
      branches: branches, branchCondition: condition)
  }

  @MainActor @Test func saysWhichWayAnIfWentAsTheCanvasNamesIt() {
    #expect(RunText.branch(Self.choice("then", of: 3, saw: "Sign in window")) == "Took “Condition 1: Sign in window”")
    #expect(RunText.branch(Self.choice("elseIf1", of: 3, saw: "Inbox is empty")) == "Took “Condition 2: Inbox is empty”")
    #expect(RunText.branch(Self.choice("else", of: 3)) == "Took “Otherwise”")
    #expect(RunText.branch(Self.choice("then", of: 2, saw: "update dialog")) == "Took “Yes: update dialog”")
    #expect(RunText.branch(Self.choice("else", of: 2)) == "Took “No”")
    // Reports from before an `if` could have more conditions.
    #expect(RunText.branch(Self.choice("then", of: nil)) == "Took “Yes”")
    #expect(RunText.branch(Self.choice("elseIf2", of: nil)) == "Took “Condition 3”")
    #expect(RunText.branch(Self.choice("next", of: nil)) == nil)
  }

  @Test func aRunWaitingForTheScreenSaysWhoHasItAndGoesOnWhenItIsFree() {
    let holder = ScreenHolder(process: "jevision", pid: 31, test: "Checkout", since: 1_700_000_000)
    var state = run([.waitingForScreen(holder: holder)])
    #expect(state.phase == .waitingForScreen(holder))
    #expect(state.isWaitingForScreen && !state.isPaused)
    state.apply(.attemptStarted(attempt: 1))
    #expect(state.phase == .running)
  }

  @MainActor @Test func theWaitNamesTheHolderOrAdmitsNotKnowingIt() {
    let holder = ScreenHolder(process: "JEVision", pid: 31, test: "Sign in", since: 1_700_000_000)
    let text = RunText.screenBusy(holder)
    #expect(text.hasPrefix("Screen busy: JEVision, test Sign in, since "))
    #expect(RunText.screenBusy(nil) == "Screen busy: another run is using it.")
    // The command-line tool is not named by its program name.
    let terminal = ScreenHolder(process: "jevision", pid: 32, test: "Checkout", since: 1_700_000_000)
    #expect(RunText.screenBusy(terminal).hasPrefix("Screen busy: JEVision in a terminal, test Checkout, since "))
    #expect(RunHUD.status(of: run([.waitingForScreen(holder: nil)])) == "Waiting for the screen")
  }

  @Test func remembersTheWaysOutTheIfsTook() {
    let state = run([
      .stepFinished(attempt: 1, step: Self.choice("elseIf1", of: 3, saw: "Inbox is empty")),
      .stepFinished(attempt: 1, step: Self.choice("then", of: 2, in: "other.flow.json")),
    ])
    #expect(state.taken == [Slot(from: "if", edge: "elseIf1")])
    #expect(state.lastChoice?.test == "other.flow.json")
    var next = state
    next.apply(.attemptStarted(attempt: 2))
    #expect(next.taken.isEmpty)
    #expect(next.lastChoice == nil)
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
    var state = run([started("a"), finished("a", .passed), .paused(block: "b", reason: .breakpoint, variables: [:])])
    #expect(state.phase == .paused(.breakpoint))
    #expect(state.isPaused)
    #expect(state.current == "b")
    state.apply(.resumed)
    #expect(state.phase == .running)
    state.apply(.paused(block: "b", reason: .userInput, variables: [:]))
    #expect(state.phase == .paused(.userInput))
  }

  @Test func keepsTheVariablesOfTheBlockAhead() {
    var state = run([
      .stepStarted(attempt: 1, seq: 1, test: "a.flow.json", block: "a", blockType: "type", variables: ["data.email": "a@x.io"], timeoutMs: nil)
    ])
    #expect(state.variables == ["data.email": "a@x.io"])
    state.apply(.paused(block: "b", reason: .breakpoint, variables: ["data.email": "b@x.io", "secret.PASSWORD": "••••"]))
    #expect(state.variables == ["data.email": "b@x.io", "secret.PASSWORD": "••••"])
    state.apply(.attemptStarted(attempt: 2))
    #expect(state.variables.isEmpty)
  }

  @Test func countsDownTheWaitOfTheBlockThatRunsAndStopsWhilePaused() {
    let start = Date(timeIntervalSince1970: 1_000)
    var state = run([])
    state.apply(started("w", type: "waitFor", timeoutMs: 6000), at: start)
    #expect(state.waitLimit == 6)
    #expect(state.remaining(at: start.addingTimeInterval(2)) == 4)
    state.apply(.paused(block: "w", reason: .requested, variables: [:]), at: start.addingTimeInterval(2))
    #expect(state.remaining(at: start.addingTimeInterval(50)) == 4)
    // The run was paused for 50 seconds: the block gets its time back.
    state.apply(.resumed, at: start.addingTimeInterval(52))
    #expect(state.remaining(at: start.addingTimeInterval(53)) == 3)
    #expect(state.remaining(at: start.addingTimeInterval(100)) == 0)
    state.apply(finished("w", .passed), at: start.addingTimeInterval(54))
    #expect(state.remaining(at: start) == nil)
    #expect(state.waitLimit == nil)
  }

  @Test func aBlockThatDoesNotWaitHasNoCountdown() {
    var state = run([started("c", timeoutMs: nil)])
    #expect(state.remaining(at: .now) == nil)
    state.apply(started("w", type: "pause", timeoutMs: 2000))
    state.apply(.attemptStarted(attempt: 2))
    #expect(state.remaining(at: .now) == nil)
    #expect(state.waitLimit == nil)
  }

  @MainActor @Test func theHUDSaysHowLongIsLeftAndWhichRetryThisIs() {
    let start = Date(timeIntervalSince1970: 1_000)
    var state = run([])
    state.apply(started("w", type: "waitFor", timeoutMs: 6000), at: start)
    #expect(RunHUD.live(of: state, at: start.addingTimeInterval(0.2)) == "6 s left")
    #expect(RunHUD.live(of: state, at: start.addingTimeInterval(2.5)) == "4 s left")
    state.apply(.attemptStarted(attempt: 2))
    #expect(RunHUD.live(of: state, at: start) == "Retry 1 of 1")
    state.apply(started("w", type: "waitFor", timeoutMs: 6000), at: start)
    #expect(RunHUD.live(of: state, at: start) == "6 s left · retry 1 of 1")
    #expect(RunHUD.liveDescription(of: state).hasSuffix("retry 1 of 1"))
  }

  @MainActor @Test func aSecretIsOnlySaidToBeSet() {
    #expect(RunHUD.shown("secret.PASSWORD", "••••") == "Set")
    #expect(RunHUD.shown("secret.PASSWORD", "hunter2") == "Set")
    #expect(RunHUD.shown("data.email", "a@x.io") == "a@x.io")
    #expect(RunHUD.shown("var.name", "") == "\u{201C}\u{201D}")
  }

  @Test func theMiniMapMarksWhatPassedFailedAndRunsNow() {
    let blocks = (0..<4).map { LaidBlock(id: "b\($0)", rect: Rect(x: 0, y: Double($0) * 100, width: 100, height: 50)) }
    let edges = (0..<3).map {
      LaidEdge(from: "b\($0)", edge: "next", to: "b\($0 + 1)", points: [Point(x: 50, y: Double($0) * 100 + 50), Point(x: 50, y: Double($0 + 1) * 100)])
    }
    let layout = FlowLayout(width: 100, height: 350, blocks: blocks, frames: [], edges: edges, slots: [], ports: [])
    let state = run([
      started("b0"), finished("b0", .passed), started("b1"), finished("b1", .failed), started("b2"),
    ])
    let map = RunMiniMap(layout: layout, state: state, in: CGSize(width: 300, height: 84))
    #expect(map.nodes.map(\.status) == [.passed, .failed, .current(paused: false), .pending])
    // The run got as far as b2; b3 is still ahead.
    #expect(map.links.map(\.reached) == [true, true, false])
    // It all fits, so nothing is cut: top to bottom, in the box.
    #expect(map.nodes.allSatisfy { (0...84).contains($0.at.y) && (0...300).contains($0.at.x) })
    #expect(map.nodes.map(\.at.y) == map.nodes.map(\.at.y).sorted())
    #expect(RunMiniMap.summary(of: layout, state: state) == "1 passed, 1 failed, 2 to go")
  }

  @Test func aLongTestShowsThePartAroundTheBlockThatRuns() {
    let blocks = (0..<50).map { LaidBlock(id: "b\($0)", rect: Rect(x: 0, y: Double($0) * 100, width: 100, height: 50)) }
    let layout = FlowLayout(width: 100, height: 5000, blocks: blocks, frames: [], edges: [], slots: [], ports: [])
    let size = CGSize(width: 300, height: 84)
    let middle = RunMiniMap(layout: layout, state: run([started("b25")]), in: size)
    #expect(abs(middle.nodes[25].at.y - 42) < 0.001)
    // At either end the map stops at the end of the test instead of showing empty space.
    let first = RunMiniMap(layout: layout, state: run([started("b0")]), in: size)
    #expect(first.nodes[0].at.y == 2.5)
    let last = RunMiniMap(layout: layout, state: run([started("b49")]), in: size)
    #expect(abs(last.nodes[49].at.y - (84 - (5000 - 4925) * RunMiniMap.smallest)) < 0.001)
  }

  @Test func blocksOfAnotherTestDontMarkThisOne() {
    var state = run([started("login", type: "runTest"), started("open", in: "shared/sign-in.flow.json")])
    #expect(state.current == nil)
    #expect(state.currentType == "click")
    state.apply(.paused(block: "type", reason: .step, variables: [:]))
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
    // Beside the block itself the message names it the way the canvas does, not by its id.
    #expect(RunText.failure(settle, blockType: "waitFor") == "Block “Wait For”: the screen didn’t settle within 10 s.")
  }

  @Test func describesClipboardAndAreaFailures() {
    let text = { (code: String, params: [String: String]) in RunText.failure(Failure(code: code, params: params)) }
    #expect(
      text("run.clipboard.notPicture", ["block": "p", "image": "refs/logo.jpg"])
        == "Block “p”: refs/logo.jpg isn’t a PNG picture, so it can’t go on the clipboard.")
    #expect(text("run.clipboard.noImage", ["block": "r"]) == "Block “r”: the clipboard holds no picture.")
    #expect(text("run.clipboard.noFiles", ["block": "r"]) == "Block “r”: the clipboard holds no copied files.")
    #expect(
      text("run.target.badArea", ["block": "c", "value": "wide"])
        == "Block “c”: “wide” isn’t an area. It must be four numbers from 0 to 1 as x,y,width,height, such as 0.1,0.2,0.5,0.3.")
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
