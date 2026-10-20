import Foundation
import func JEVisionMobile.currentLanguage
import LocalAuthentication
import Testing

@testable import JEVisionRemote

struct PairingLinkTests {
  @Test func readsTheMacsCode() throws {
    let link = try #require(PairingLink(pairingText(name: "Kate%E2%80%99s%20MacBook")))
    #expect(link.macName == "Kate’s MacBook")
    #expect(link.expiresAt == Date(timeIntervalSince1970: 4_000_000_000))
    #expect(link.text == pairingText(name: "Kate%E2%80%99s%20MacBook"))
  }

  @Test func acceptsAnOpenedURLAndSurroundingSpace() {
    #expect(PairingLink(URL(string: pairingText())!) != nil)
    #expect(PairingLink("  " + pairingText().replacingOccurrences(of: "jevision://pair", with: "JEVision://PAIR") + "\n") != nil)
  }

  @Test(arguments: [
    "https://example.com/pair?v=1",
    "jevision://open?v=1",
    pairingText().replacingOccurrences(of: "v=1", with: "v=2"),
    pairingText().replacingOccurrences(of: "&name=Studio", with: ""),
    pairingText().replacingOccurrences(of: "name=Studio", with: "name=%20"),
    pairingText().replacingOccurrences(of: "0f0f", with: "zz0f"),
    pairingText().replacingOccurrences(of: "exp=4000000000", with: "exp=soon"),
    "",
  ])
  func rejectsOtherCodes(_ text: String) {
    #expect(PairingLink(text) == nil)
  }

  @Test func knowsWhenItExpired() throws {
    let link = try #require(PairingLink(pairingText(exp: 100)))
    #expect(!link.isExpired(at: Date(timeIntervalSince1970: 99)))
    #expect(link.isExpired(at: Date(timeIntervalSince1970: 100)))
  }
}

struct LockPolicyTests {
  let start = Date(timeIntervalSince1970: 0)

  @Test func locksAfterTheTimeoutInTheBackground() {
    let policy = LockPolicy(timeout: .oneMinute)
    let away = start + 100
    #expect(!policy.mustLock(unlockedAt: start, backgroundedAt: away, now: away + 59))
    #expect(policy.mustLock(unlockedAt: start, backgroundedAt: away, now: away + 60))
  }

  @Test func immediatelyMeansAnyTimeAway() {
    #expect(LockPolicy(timeout: .immediately).mustLock(unlockedAt: start, backgroundedAt: start + 5, now: start + 5))
  }

  @Test func staysOpenInTheForegroundUntilTheSessionEnds() {
    let policy = LockPolicy(timeout: .immediately)
    #expect(!policy.mustLock(unlockedAt: start, backgroundedAt: nil, now: start + 7 * 3600))
    #expect(policy.mustLock(unlockedAt: start, backgroundedAt: nil, now: start + 8 * 3600))
  }
}

@MainActor
struct AppLockTests {
  @Test func unlocksAndLocksAgainAfterTheTimeout() async {
    let clock = Clock()
    let lock = makeLock(clock: clock)
    var contexts: [Bool] = []
    lock.onChange = { contexts.append($0 != nil) }
    lock.timeout = .fiveMinutes

    await lock.unlock(reason: "test")
    #expect(lock.isUnlocked)

    lock.didEnterBackground()
    clock.advance(299)
    lock.checkExpiry()
    #expect(lock.isUnlocked)

    lock.didEnterBackground()
    clock.advance(300)
    lock.checkExpiry()
    #expect(!lock.isUnlocked)
    #expect(lock.context == nil)
    #expect(contexts == [true, false])
  }

  @Test func cancellingLeavesItLockedWithoutComplaint() async {
    let lock = AppLock(defaults: UserDefaults(suiteName: UUID().uuidString)!, authenticate: { _ in throw LAError(.userCancel) })
    await lock.unlock(reason: "test")
    #expect(lock.state == .locked)
    #expect(lock.problem == nil)
  }

  @Test func saysSoWhenThereIsNoPasscode() async {
    let lock = AppLock(defaults: UserDefaults(suiteName: UUID().uuidString)!, authenticate: { _ in throw LAError(.passcodeNotSet) })
    await lock.unlock(reason: "test")
    #expect(lock.problem == .noPasscode)
  }

  @Test func remembersTheTimeout() {
    let defaults = UserDefaults(suiteName: UUID().uuidString)!
    AppLock(defaults: defaults).timeout = .fifteenMinutes
    #expect(AppLock(defaults: defaults).timeout == .fifteenMinutes)
    #expect(makeLock().timeout == .oneMinute)
  }
}

@MainActor
struct AppModelTests {
  @Test func pairingSavesTheMacAndKeepsTheConnection() async throws {
    let store = makeStore()
    let model = AppModel(client: FakeClient(), store: store, lock: makeLock())
    #expect(!model.needsUnlock)

    let host = try await model.pair(try #require(PairingLink(pairingText())), deviceName: "iPhone")

    #expect(host == macA)
    #expect(model.hosts == [macA])
    #expect(store.load() == [macA])
    #expect(model.lock.isUnlocked)
    #expect(model.permission(on: macA) == .run)
  }

  @Test func anExpiredCodeIsNotSentToTheMac() async throws {
    let client = FakeClient()
    let model = AppModel(client: client, store: makeStore(), lock: makeLock())
    let link = try #require(PairingLink(pairingText(exp: 100)))
    await #expect(throws: RemoteError.codeExpired) {
      try await model.pair(link, deviceName: "iPhone")
    }
    #expect(client.pairs.withLock { $0 } == 0)
  }

  @Test func aRefusedPairingAddsNothing() async throws {
    let client = FakeClient()
    client.failure.withLock { $0 = .invalidCode }
    let model = AppModel(client: client, store: makeStore(), lock: makeLock())
    await #expect(throws: RemoteError.invalidCode) {
      try await model.pair(try #require(PairingLink(pairingText())), deviceName: "iPhone")
    }
    #expect(model.hosts.isEmpty)
  }

  @Test func aSavedMacNeedsUnlockingFirst() async throws {
    let store = makeStore()
    try store.save([macA])
    let model = AppModel(client: FakeClient(), store: store, lock: makeLock())
    #expect(model.needsUnlock)
    await #expect(throws: RemoteError.other) { try await model.session(for: macA) }
    await model.lock.unlock(reason: "test")
    #expect(!model.needsUnlock)
  }

  @Test func connectsOnceAndSavesFreshAddresses() async throws {
    let store = makeStore()
    try store.save([macA])
    let client = FakeClient(delay: .milliseconds(50))
    let model = AppModel(client: client, store: store, lock: makeLock())
    await model.lock.unlock(reason: "test")

    async let first = model.session(for: macA)
    async let second = model.session(for: macA)
    let sessions = try await [first, second]

    #expect(sessions[0] === sessions[1])
    #expect(client.connects.withLock { $0 } == 1)
    #expect(model.hosts.first?.addrs == ["192.168.1.20:52114"])
    #expect(store.load().first?.addrs == ["192.168.1.20:52114"])
  }

  @Test func anOfflineMacIsShownAndTriedAgainLater() async throws {
    let store = makeStore()
    try store.save([macA])
    let client = FakeClient()
    client.failure.withLock { $0 = .macOffline }
    let model = AppModel(client: client, store: store, lock: makeLock())
    await model.lock.unlock(reason: "test")

    let failed = await model.fetch(macA) { session throws(RemoteError) in try await session.listProjects() }
    guard case .failed(.macOffline) = failed else { Issue.record("expected macOffline, got \(failed)"); return }
    guard case .unreachable(.macOffline) = model.connections[macA.id] else { Issue.record("not marked offline"); return }

    client.failure.withLock { $0 = nil }
    let projects = await model.fetch(macA) { session throws(RemoteError) in try await session.listProjects() }
    #expect(projects.value == [Project(name: "Notes")])
    #expect(client.connects.withLock { $0 } == 2)
  }

  @Test func forgettingTheLastMacClosesItAndLocks() async throws {
    let store = makeStore()
    let model = AppModel(client: FakeClient(), store: store, lock: makeLock())
    _ = try await model.pair(try #require(PairingLink(pairingText())), deviceName: "iPhone")
    let session = try #require(try await model.session(for: macA) as? FakeSession)

    await model.forget(macA)

    #expect(model.hosts.isEmpty)
    #expect(store.load().isEmpty)
    #expect(model.connections.isEmpty)
    #expect(session.closed.withLock { $0 })
    #expect(!model.lock.isUnlocked)
  }

  @Test func lockingDropsEveryConnection() async throws {
    let model = AppModel(client: FakeClient(), store: makeStore(), lock: makeLock())
    model.lock.onChange = { [model] context in if context == nil { model.disconnectAll() } }
    _ = try await model.pair(try #require(PairingLink(pairingText())), deviceName: "iPhone")
    #expect(model.connections.count == 1)

    model.lock.lock()

    #expect(model.connections.isEmpty)
    #expect(model.needsUnlock)
  }
}

struct RunProgressTests {
  private func status(
    _ state: RunState, block: String? = nil, attempt: Int = 1, verdict: Verdict? = nil, waiting: ScreenWait? = nil
  ) -> RunUpdate {
    .status(
      RunStatus(run: "r", test: "main.json", state: state, verdict: verdict, attempt: attempt, block: block, waiting: waiting))
  }

  @Test func waitsForTheScreenUntilItsFirstBlockStarts() {
    let holder = ScreenHolder(process: "jevision", test: "Sign in", since: Date(timeIntervalSince1970: 1_800_000_000))
    var progress = RunProgress()
    progress.apply(status(.running))
    #expect(progress.waiting == nil)
    progress.apply(status(.running, waiting: ScreenWait(holder: holder)))
    #expect(progress.waiting == ScreenWait(holder: holder))
    #expect(progress.state == .running)
    progress.apply(.event(.blockStarted(test: "main.json", block: "a", kind: "click")))
    #expect(progress.waiting == nil)

    // A retry waits again, and a run that ends doesn't wait any more.
    progress.apply(.event(.attemptStarted(attempt: 2)))
    progress.apply(status(.running, attempt: 2, waiting: ScreenWait(holder: nil)))
    #expect(progress.waiting == ScreenWait(holder: nil))
    progress.apply(.event(.finished(verdict: .cancelled, flaky: false)))
    #expect(progress.waiting == nil)
  }

  @Test func aStatusFromAnOlderMacIsNotWaiting() {
    var progress = RunProgress()
    progress.apply(status(.running, waiting: ScreenWait(holder: nil)))
    progress.apply(status(.running))
    #expect(progress.waiting == nil)
  }

  @Test func followsBlocksToTheEnd() {
    var progress = RunProgress()
    let failure = Failure(code: "run.target.notFound", params: ["block": "b"])
    progress.apply(status(.running, block: "a"))
    #expect(progress.current == "a")
    progress.apply(.event(.blockFinished(test: "main.json", block: "a", status: .passed, failure: nil)))
    progress.apply(.event(.blockStarted(test: "main.json", block: "b", kind: "click")))
    #expect(progress.current == "b")
    progress.apply(.event(.blockFinished(test: "main.json", block: "b", status: .failed, failure: failure)))
    progress.apply(.event(.finished(verdict: .failed, flaky: false)))

    #expect(progress.marks == ["a": .passed, "b": .failed])
    #expect(progress.steps.map(\.kind) == ["", "click"])
    #expect(progress.failure == failure)
    #expect(progress.isFinished)
    #expect(progress.verdict == .failed)
    #expect(progress.current == nil)
  }

  @Test func keepsWhatABlockWorksWithAsTheMacFilledItIn() {
    var progress = RunProgress()
    progress.apply(status(.running))
    progress.apply(.event(.blockStarted(test: "main.json", block: "greet", kind: "type", title: "Hello, Boris!")))
    #expect(progress.title(of: "greet") == "Hello, Boris!")
    // A block of a called test isn't this test's, and a retry starts clean.
    progress.apply(.event(.blockStarted(test: "login.json", block: "greet", kind: "type", title: "Hi")))
    #expect(progress.title(of: "greet") == "Hello, Boris!")
    progress.apply(.event(.attemptStarted(attempt: 2)))
    #expect(progress.title(of: "greet") == nil)
  }

  @Test func blocksOfACalledTestAreStepsButNotMarks() {
    var progress = RunProgress()
    progress.apply(status(.running))
    progress.apply(.event(.blockStarted(test: "login.json", block: "a", kind: "type")))
    progress.apply(.event(.blockFinished(test: "login.json", block: "a", status: .passed, failure: nil)))
    #expect(progress.marks.isEmpty)
    #expect(progress.current == nil)
    #expect(progress.steps.count == 1)
  }

  @Test func aRetryStartsClean() {
    var progress = RunProgress()
    progress.apply(status(.running))
    progress.apply(
      .event(.blockFinished(test: "main.json", block: "a", status: .failed, failure: Failure(code: "x", params: [:]))))
    progress.apply(.event(.attemptStarted(attempt: 2)))
    #expect(progress.attempt == 2)
    #expect(progress.marks.isEmpty)
    #expect(progress.steps.isEmpty)
    #expect(progress.failure == nil)
  }

  @Test func listsStepsInRunOrderWithTheRunningOneLast() {
    var progress = RunProgress()
    progress.apply(status(.running, block: "open"))
    #expect(progress.lines.map(\.block) == ["open"])
    #expect(progress.lines.last?.status == nil)
    progress.apply(.event(.blockFinished(test: "main.json", block: "open", status: .passed, failure: nil)))
    progress.apply(.event(.blockStarted(test: "main.json", block: "ready", kind: "if")))
    progress.apply(
      .event(
        .blockFinished(
          test: "main.json", block: "ready", status: .passed, failure: nil, branch: "then", branches: 2,
          branchCondition: "An empty document")))
    progress.apply(.event(.blockStarted(test: "main.json", block: "rows", kind: "useData")))
    progress.apply(.event(.blockFinished(test: "main.json", block: "rows", status: .passed, failure: nil)))
    progress.apply(.event(.blockStarted(test: "main.json", block: "type", kind: "type")))
    let running = progress.lines.last

    #expect(progress.lines.map(\.block) == ["open", "ready", "rows", "type"], "Oldest first")
    #expect(progress.lines.map(\.id) == [0, 1, 2, 3])
    #expect(running?.status == nil, "The running step is last")
    #expect(progress.lines[1].took != nil)
    #expect(progress.lines[1].took == RunText.branch("then", branches: 2, condition: "An empty document"))
    #expect(progress.lines[2].took == nil)

    progress.apply(.event(.blockFinished(test: "main.json", block: "type", status: .passed, failure: nil)))
    #expect(progress.lines.last?.id == running?.id, "A step keeps its place when it finishes")
    #expect(progress.lines.last?.status == .passed)
    progress.apply(.event(.finished(verdict: .passed, flaky: false)))
    #expect(progress.lines.count == 4)
  }

  @Test func pausesAndResumes() {
    var progress = RunProgress()
    progress.apply(status(.running))
    progress.apply(.event(.paused(block: "c")))
    #expect(progress.state == .paused)
    #expect(progress.current == "c")
    progress.apply(.event(.resumed))
    #expect(progress.state == .running)
  }

  @Test func aLateWatcherStartsFromTheStatus() {
    var progress = RunProgress()
    progress.apply(status(.finished, block: "z", attempt: 2, verdict: .passed))
    #expect(progress.isFinished)
    #expect(progress.verdict == .passed)
    #expect(progress.attempt == 2)
    #expect(progress.current == nil)
  }
}

struct TestTreeTests {
  private func entry(_ path: String, folder: Bool = false) -> TestEntry {
    TestEntry(path: path, name: String(path.split(separator: "/").last!), isFolder: folder)
  }

  @Test func listsDirectChildrenFoldersFirst() {
    let entries = [
      entry("b.json"), entry("A.json"), entry("login", folder: true), entry("login/ok.json"),
      entry("deep/er/x.json"),
    ]
    #expect(TestTree.children(of: "", in: entries).map(\.path) == ["deep", "login", "A.json", "b.json"])
    #expect(TestTree.children(of: "login", in: entries).map(\.path) == ["login/ok.json"])
    #expect(TestTree.children(of: "deep", in: entries).map(\.path) == ["deep/er"])
    #expect(TestTree.children(of: "deep", in: entries).first?.isFolder == true)
  }

  @Test func doesNotMistakeAPrefixForAFolder() {
    let entries = [entry("log.json"), entry("login/ok.json")]
    #expect(TestTree.children(of: "log", in: entries).isEmpty)
  }
}

struct ReportTitleTests {
  private func attempt(_ number: Int, row: RunReport.DataRow? = nil) -> RunReport.Attempt {
    .init(number: number, verdict: .passed, durationMs: 1, failure: nil, row: row, steps: [])
  }

  private func report(_ attempts: [RunReport.Attempt]) -> RunReport {
    RunReport(
      test: "T", file: "t.flow.json", app: "a", startedAt: .now, durationMs: 1, verdict: .passed, flaky: false,
      baselinesCreated: [], attempts: attempts)
  }

  @Test func namesTriesOnlyWhenThereAreSeveral() {
    let one = report([attempt(1)])
    #expect(ReportView.title(of: one.attempts[0], in: one) == nil)
    let two = report([attempt(1), attempt(2)])
    #expect(ReportView.title(of: two.attempts[1], in: two)?.contains("2") == true)
  }

  @Test func namesTheRowOfEachAttempt() {
    let anna = RunReport.DataRow(number: 1, label: "Anna")
    let boris = RunReport.DataRow(number: 2, label: "Boris")
    let rows = report([attempt(1, row: anna), attempt(1, row: boris)])
    #expect(ReportView.title(of: rows.attempts[1], in: rows) == TestDetailView.rowTitle(number: 2, label: "Boris"))
    let retried = report([attempt(1, row: anna), attempt(2, row: anna)])
    let title = ReportView.title(of: retried.attempts[1], in: retried)
    #expect(title?.hasPrefix(TestDetailView.rowTitle(number: 1, label: "Anna")) == true)
    #expect(title != ReportView.title(of: retried.attempts[0], in: retried))
  }
}

struct CardAndQueueTextTests {
  private func flow(priority: String? = nil, testType: String? = nil, status: String? = nil, author: String? = nil) -> TestFlow {
    TestFlow(
      name: "Sign in", description: nil, app: "com.example.app", start: "open", size: .zero, blocks: [], frames: [], edges: [],
      priority: priority, testType: testType, status: status, author: author)
  }

  @Test func theCardReadsLikeTheMacsInItsOrder() {
    let facts = CardText.facts(of: flow(priority: "critical", testType: "smoke", status: "ready", author: "Anna"))
    #expect(facts.map(\.label) == [String(localized: "Type"), String(localized: "Priority"), String(localized: "Status"), String(localized: "Author")])
    #expect(facts.map(\.value) == [String(localized: "Smoke"), String(localized: "Critical"), String(localized: "Ready"), "Anna"])
  }

  @Test func aCardWithoutFieldsShowsNone() {
    #expect(CardText.facts(of: flow()).isEmpty)
    #expect(CardText.facts(of: flow(author: "")).isEmpty)
  }

  @Test func aValueFromANewerMacShowsAsWritten() {
    #expect(CardText.priority("urgent") == "urgent")
    #expect(CardText.testType("load") == "load")
    #expect(CardText.status("retired") == "retired")
  }

  @Test func theQueueSaysWhoHasTheScreen() {
    let since = Date(timeIntervalSince1970: 1_800_000_000)
    let holder = ScreenHolder(process: "jevision", test: "Sign in", since: since)
    let time = since.formatted(date: .omitted, time: .shortened)
    CoreLanguage.apply(["en"])
    #expect(RunText.screenBusy(holder) == "Screen busy: jevision, test Sign in, since \(time)")
    #expect(RunText.screenBusy(nil) == "Screen busy: another run is using it.")
  }

  @Test func theCoreWordsFailuresInTheLanguageOfThePhone() {
    CoreLanguage.apply(["en-GB"])
    #expect(currentLanguage() == "en")
    let failure = Failure(code: "run.target.notFound", params: ["block": "open", "image": "x.png"])
    #expect(RunText.failure(failure) == "Block “open”: x.png wasn’t found on the screen.")
    #expect(RunText.branch("then", branches: 2, condition: nil) == "Took “Yes”")
    #expect(RunText.branch("then", branches: 3, condition: "An empty document") == "Took “Condition 1: An empty document”")
    #expect(VisionText.failure(Failure(code: "run.vision.noModel", params: ["provider": "Ollama"])).contains("Ollama"))
  }
}
