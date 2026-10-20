import Foundation
import JEVisionCore
import Testing

@testable import JEVision

struct RunHistoryTests {
  private static func attempt(_ number: UInt32, _ verdict: Verdict, failure: String? = nil, steps: [Step] = []) -> Attempt {
    Attempt(
      number: number, verdict: verdict, durationMs: 1000, failure: failure.map { Failure(code: $0, params: [:]) },
      steps: steps)
  }

  private static func run(
    _ test: String, at time: String, _ verdict: Verdict = .passed, flaky: Bool = false, attempts: [Attempt]? = nil,
    dir: String? = nil
  ) -> SavedRun {
    SavedRun(
      dir: dir ?? "/p/.jevision/runs/\(time)-\(test)",
      report: RunReport(
        test: test, file: test + ".flow.json", app: "com.example.App", startedAt: time, durationMs: 1500,
        verdict: verdict, flaky: flaky, baselinesCreated: [], attempts: attempts ?? [attempt(1, verdict)]))
  }

  private static var utc: Calendar {
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = TimeZone(identifier: "UTC")!
    return calendar
  }

  @Test func readsReportTimes() {
    #expect(abs(RunHistory.date("2026-10-18T14:30:05.123Z").timeIntervalSince1970 - 1_792_333_805.123) < 0.0005)
    #expect(RunHistory.date("2026-10-18T14:30:05Z").timeIntervalSince1970 == 1_792_333_805)
    #expect(RunHistory.date("yesterday") == .distantPast)
  }

  @Test func groupsByDayNewestFirst() {
    let runs = [
      Self.run("login", at: "2026-10-17T23:59:00.000Z"),
      Self.run("home", at: "2026-10-18T09:00:00.000Z"),
      Self.run("login", at: "2026-10-18T14:00:00.000Z", .failed),
      Self.run("login", at: "2026-10-16T08:00:00.000Z"),
      // Two runs in the same millisecond keep a stable order: by folder.
      Self.run("home", at: "2026-10-18T09:00:00.000Z", dir: "/p/.jevision/runs/b"),
    ]
    let days = RunHistory.days(runs, calendar: Self.utc)
    #expect(days.map { $0.runs.count } == [3, 1, 1])
    #expect(days[0].runs.map(\.report.startedAt) == ["2026-10-18T14:00:00.000Z", "2026-10-18T09:00:00.000Z", "2026-10-18T09:00:00.000Z"])
    #expect(days[0].runs[1].dir == "/p/.jevision/runs/b")
    #expect(days[1].runs[0].report.startedAt == "2026-10-17T23:59:00.000Z")
    #expect(days.map(\.day) == days.map(\.day).sorted(by: >))

    let login = RunHistory.days(runs, matching: " LOG ", calendar: Self.utc)
    #expect(login.flatMap(\.runs).allSatisfy { $0.report.test == "login" })
    #expect(login.flatMap(\.runs).count == 3)
    #expect(RunHistory.days(runs, matching: "nothing").isEmpty)
  }

  @Test func namesTheDays() {
    let now = RunHistory.date("2026-10-18T12:00:00Z")
    let title = { RunHistory.title(of: RunHistory.date($0), now: now, calendar: Self.utc) }
    #expect(title("2026-10-18T01:00:00Z") == String(localized: "Today"))
    #expect(title("2026-10-17T23:00:00Z") == String(localized: "Yesterday"))
    #expect(title("2026-10-12T10:00:00Z").contains("12"))
    #expect(title("2025-10-12T10:00:00Z").contains("2025"))
  }

  @Test func remembersHowEachTestLastEnded() {
    let latest = ProjectMap.latestRuns([
      Self.run("login", at: "2026-10-18T09:00:00.000Z"),
      Self.run("login", at: "2026-10-18T14:00:00.000Z", .failed),
      Self.run("home", at: "2026-10-18T10:00:00.000Z", .passed, flaky: true),
      Self.run("home", at: "2026-10-17T10:00:00.000Z", .failed),
      Self.run("draft", at: "2026-10-18T10:00:00.000Z", .cancelled),
    ])
    #expect(latest.mapValues(\.status) == ["login.flow.json": .failed, "home.flow.json": .flaky, "draft.flow.json": .cancelled])
  }

  @Test func tellsARealFailureFromAFlakyTest() {
    let once = Self.run("a", at: "2026-10-18T09:00:00Z", .failed)
    #expect(RunHistory.retryVerdict(once.report) == nil)

    let real = Self.run(
      "a", at: "2026-10-18T09:00:00Z", .failed,
      attempts: [1, 2, 3].map { Self.attempt($0, .failed, failure: "run.check.mismatch") })
    #expect(RunHistory.retryVerdict(real.report) == "Failed 3 times in a row — a real failure, not flakiness.")
    #expect(real.status == .failed)
    var quarantined = real
    quarantined.report.quarantined = true
    #expect(quarantined.status == .quarantined)

    let flakyPass = Self.run(
      "a", at: "2026-10-18T09:00:00Z", .passed, flaky: true,
      attempts: [Self.attempt(1, .failed, failure: "run.waitFor.timeout"), Self.attempt(2, .failed, failure: "run.waitFor.timeout"), Self.attempt(3, .passed)])
    #expect(RunHistory.retryVerdict(flakyPass.report) == "Passed on retry 2 after failing — the test is flaky.")
    #expect(flakyPass.status == .flaky)

    let mixed = Self.run(
      "a", at: "2026-10-18T09:00:00Z", .failed, flaky: true,
      attempts: [Self.attempt(1, .failed, failure: "run.waitFor.timeout"), Self.attempt(2, .failed, failure: "run.check.mismatch")])
    #expect(RunHistory.retryVerdict(mixed.report) == "Failed 2 of 2 times, each time differently — the test may be flaky.")
    #expect(mixed.status == .failed)
  }

  @Test func findsTheInnermostFailedStep() {
    func step(_ seq: UInt32, _ block: String, _ status: StepStatus, failed: Bool) -> Step {
      Step(
        seq: seq, test: "a.flow.json", block: block, blockType: "repeat", status: status, durationMs: 0, before: nil,
        after: nil, expected: nil, diff: nil, reference: nil, similarity: nil, vision: nil, branch: nil,
        failure: failed ? Failure(code: "run.target.notFound", params: [:]) : nil)
    }
    let report = Self.run(
      "a", at: "2026-10-18T09:00:00Z", .failed,
      attempts: [Self.attempt(1, .failed, steps: [step(1, "open", .passed, failed: false), step(2, "loop", .failed, failed: false), step(3, "click", .failed, failed: true)])]
    ).report
    #expect(RunHistory.failedStep(report)?.block == "click")
    #expect(RunHistory.failedStep(Self.run("b", at: "2026-10-18T09:00:00Z").report) == nil)
  }

  private static func ci(pullRequest: UInt64? = nil, branch: String? = "main", url: String? = nil) -> CiOrigin {
    CiOrigin(
      system: "GitHub Actions", repository: "acme/qa", branch: branch, commit: "abc1234def", pullRequest: pullRequest,
      runUrl: url)
  }

  private static func started(_ run: SavedRun, by change: (inout RunReport) -> Void) -> SavedRun {
    var report = run.report
    change(&report)
    return SavedRun(dir: run.dir, report: report)
  }

  @Test func narrowingTheListClosesTheReportOfARunItHides() {
    let base = Self.run("login", at: "2026-10-18T09:00:00.000Z")
    let ci = SavedRun(dir: "/p/.jevision/runs/ci", report: Self.started(base) { $0.ci = Self.ci() }.report)
    let local = SavedRun(dir: "/p/.jevision/runs/local", report: base.report)
    let runs = [ci, local]
    #expect(RunHistory.selection(ci.id, in: runs, from: nil) == ci.id)
    #expect(RunHistory.selection(ci.id, in: runs, from: .ci) == ci.id)
    #expect(RunHistory.selection(ci.id, in: runs, from: .schedule) == nil)
    #expect(RunHistory.selection(local.id, in: runs, from: .thisMac) == local.id)
    #expect(RunHistory.selection(nil, in: runs, from: .ci) == nil)
  }

  @Test func runsAreToldApartByWhereTheyWereStarted() {
    let base = Self.run("login", at: "2026-10-18T09:00:00.000Z")
    let phone = Self.started(base) { $0.requestedBy = "Anna’s iPhone" }
    let schedule = Self.started(base) { $0.schedule = "Nightly" }
    let ci = Self.started(base) { $0.ci = Self.ci(pullRequest: 482) }
    // A report from CI says so even when it also names a schedule.
    let both = Self.started(base) {
      $0.ci = Self.ci()
      $0.schedule = "Nightly"
    }
    #expect([base, phone, schedule, ci, both].map(\.source) == [.thisMac, .phone, .schedule, .ci, .ci])

    let runs = [base, phone, schedule, ci, both].enumerated().map { index, run in
      SavedRun(dir: "/p/.jevision/runs/\(index)", report: run.report)
    }
    let shown = { (source: RunSource?) in RunHistory.days(runs, from: source, calendar: Self.utc).flatMap(\.runs).map(\.dir) }
    #expect(shown(nil).count == 5)
    #expect(shown(.thisMac) == ["/p/.jevision/runs/0"])
    #expect(shown(.phone) == ["/p/.jevision/runs/1"])
    #expect(shown(.schedule) == ["/p/.jevision/runs/2"])
    #expect(Set(shown(.ci)) == ["/p/.jevision/runs/3", "/p/.jevision/runs/4"])
    #expect(RunHistory.days(runs, matching: "nothing", from: .ci).isEmpty)
    #expect(RunHistory.days(runs, matching: "login", from: .ci).flatMap(\.runs).count == 2)
  }

  @Test func rowsNameTheSourceOfTheRun() {
    var report = Self.run("login", at: "2026-10-18T09:00:00.000Z").report
    #expect(RunHistory.origin(report) == "This Mac")
    report.schedule = "Nightly"
    #expect(RunHistory.origin(report) == "Schedule · Nightly")
    report.requestedBy = "Anna’s iPhone"
    #expect(RunHistory.origin(report) == "Anna’s iPhone")
    report.ci = Self.ci(pullRequest: 482, branch: "feat/login")
    #expect(RunHistory.origin(report) == "CI · pull request #482")
    report.ci = Self.ci(branch: "main")
    #expect(RunHistory.origin(report) == "CI · branch main")
    report.ci = Self.ci(branch: nil)
    #expect(RunHistory.origin(report) == "CI")
  }

  @Test func onlyAWebAddressOfTheCIRunIsOpened() {
    #expect(RunHistory.link(Self.ci(url: "https://github.com/acme/qa/actions/runs/9")) == URL(string: "https://github.com/acme/qa/actions/runs/9"))
    for unsafe in ["file:///etc/passwd", "javascript:alert(1)", "http://github.com/acme/qa", "not a url", "https:///nohost", ""] {
      #expect(RunHistory.link(Self.ci(url: unsafe)) == nil, "\(unsafe)")
    }
    #expect(RunHistory.link(Self.ci(url: nil)) == nil)
  }

  @MainActor @Test func theFetchOfCIRunsEndsInSomethingToSayNotAnAlert() {
    let found = CiRefresh(imported: 2, artifacts: 3, failed: 0)
    #expect(ProjectCI.state(after: .success(found)) == .done(found))
    #expect(ProjectCI.state(after: .failure(GitError.GitHubSignedOut)) == .needsSignIn)
    for notThere: GitError in [.NotInstalled, .NotRepository, .NoRemote, .NotGitHub] {
      #expect(ProjectCI.state(after: .failure(notThere)) == .notOnGitHub)
    }
    guard case .failed(let message) = ProjectCI.state(after: .failure(GitError.Unreachable)) else {
      Issue.record("an unreachable GitHub is a failure")
      return
    }
    #expect(!message.isEmpty)
    #expect(ProjectCI.problem(in: .needsSignIn) == nil)
    #expect(ProjectCI.problem(in: .notOnGitHub) == nil)
    #expect(ProjectCI.problem(in: .done(found)) == nil)
    #expect(ProjectCI.problem(in: .done(CiRefresh(imported: 0, artifacts: 1, failed: 1))) != nil)
    #expect(ProjectCI.problem(in: .failed("No network")) == "No network")
  }

  @Test func ciRunsDoNotChangeWhatTheTestsHereShow() {
    let local = Self.run("login", at: "2026-10-18T09:00:00.000Z", .failed)
    let fromCI = Self.started(Self.run("login", at: "2026-10-18T12:00:00.000Z")) { $0.ci = Self.ci() }
    let history = ProjectMap.latestRuns([local, fromCI].filter { $0.report.ci == nil })
    #expect(history["login.flow.json"]?.report.verdict == .failed)
  }
}

struct BaselineTests {
  /// A project folder with a reference and a run's screenshot, and a Trash of its own.
  private struct Folder {
    let root = FileManager.default.temporaryDirectory.appending(path: "jevision-baseline-\(UUID().uuidString)")
    var reference: URL { root.appending(path: "refs/home.png") }
    var shot: URL { root.appending(path: ".jevision/runs/r/shots/1-001-check-after.png") }
    var trash: URL { root.appending(path: "Trash") }

    init() throws {
      for dir in [reference, shot, trash.appending(path: "x")] {
        try FileManager.default.createDirectory(at: dir.deletingLastPathComponent(), withIntermediateDirectories: true)
      }
      try Data("old".utf8).write(to: reference)
      try Data("new".utf8).write(to: shot)
    }

    func moveToTrash(_ url: URL) throws -> URL {
      let target = trash.appending(path: url.lastPathComponent)
      try FileManager.default.moveItem(at: url, to: target)
      return target
    }

    func text(_ url: URL) -> String? { try? String(contentsOf: url, encoding: .utf8) }
  }

  @Test func acceptsAndTakesBack() throws {
    let folder = try Folder()
    defer { try? FileManager.default.removeItem(at: folder.root) }
    #expect(!Baseline.isReference(folder.shot, folder.reference))

    let old = try Baseline.accept(folder.shot, as: folder.reference, trash: folder.moveToTrash)
    #expect(folder.text(folder.reference) == "new")
    #expect(old.map(folder.text) == "old")
    #expect(folder.text(folder.shot) == "new", "the run keeps its screenshot")
    #expect(Baseline.isReference(folder.shot, folder.reference))

    try Baseline.restore(old, to: folder.reference)
    #expect(folder.text(folder.reference) == "old")
    #expect(!FileManager.default.fileExists(atPath: folder.trash.appending(path: "home.png").path))
  }

  @Test func acceptsWhenThereWasNoReference() throws {
    let folder = try Folder()
    defer { try? FileManager.default.removeItem(at: folder.root) }
    let missing = folder.root.appending(path: "refs/dark/new.png")
    let old = try Baseline.accept(folder.shot, as: missing, trash: folder.moveToTrash)
    #expect(old == nil)
    #expect(folder.text(missing) == "new")
    try Baseline.restore(old, to: missing)
    #expect(!FileManager.default.fileExists(atPath: missing.path))
  }

  @Test func putsTheOldReferenceBackWhenTheCopyFails() throws {
    let folder = try Folder()
    defer { try? FileManager.default.removeItem(at: folder.root) }
    let gone = folder.root.appending(path: "no-such-shot.png")
    #expect(throws: (any Error).self) { try Baseline.accept(gone, as: folder.reference, trash: folder.moveToTrash) }
    #expect(folder.text(folder.reference) == "old")
  }

  @Test func staysInsideTheProject() {
    let root = URL(filePath: "/p/project", directoryHint: .isDirectory)
    #expect(Baseline.file("refs/a.png", in: root)?.path == "/p/project/refs/a.png")
    #expect(Baseline.file("refs/../../a.png", in: root) == nil)
    #expect(Baseline.file("../project-other/a.png", in: root) == nil)
  }
}

struct RunVideoViewTests {
  @Test func clockShowsMinutesAndSeconds() {
    #expect(RunVideoView.clock(0) == "0:00")
    #expect(RunVideoView.clock(61.9) == "1:01")
    #expect(RunVideoView.clock(.nan) == "0:00")
  }

  @Test func reportsSayWhereTheyRan() {
    var report = RunReport(
      test: "login", file: "login.flow.json", app: "com.example.App", startedAt: "2026-10-18T09:00:00.000Z",
      durationMs: 1500, verdict: .passed, flaky: false, baselinesCreated: [], attempts: [])
    #expect(report.machine.isEmpty)
    report.virtualMachine = true
    report.displayScale = 1
    #expect(report.machine == ["Virtual machine", "Standard display (1×)"])
    report.virtualMachine = false
    report.displayScale = 2
    #expect(report.machine == ["Physical Mac", "Retina display (2×)"])
  }
}
