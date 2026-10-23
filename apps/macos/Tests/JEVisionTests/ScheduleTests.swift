import Foundation
import JEVisionCore
import JEVisionHands
import Testing

@testable import JEVision
@testable import JEVisionCLI

private func scratch() throws -> URL {
  let made = FileManager.default.temporaryDirectory.appending(path: "jevision-schedule-\(UUID().uuidString)")
  try FileManager.default.createDirectory(at: made, withIntermediateDirectories: true)
  let real = try #require(realpath(made.path, nil))
  defer { free(real) }
  return URL(filePath: String(cString: real))
}

private func daily(_ name: String, at hour: UInt8, _ minute: UInt8) -> ProjectSchedule {
  ProjectSchedule(name: name, tests: .all, repeats: .daily, hour: hour, minute: minute, environment: nil, video: nil)
}

struct LocalMinuteTests {
  @Test func roundTrip() throws {
    let zone = try #require(TimeZone(identifier: "Europe/Moscow"))
    let date = Date(timeIntervalSince1970: 1_800_000_000)
    let minute = LocalMinute.of(date, in: zone)
    #expect(minute == (1_800_000_000 + 3 * 3600) / 60)
    #expect(LocalMinute.date(minute, in: zone) == Date(timeIntervalSince1970: Double(minute * 60 - 3 * 3600)))
  }
}

struct ScheduleCommandTests {
  @Test func parsesScheduleRun() throws {
    #expect(try Command.parse(["schedule", "run", "p"]) == .scheduleRun(project: "p"))
    #expect(try Command.parse(["schedule"]) == .help("schedule"))
    #expect(throws: UsageError.self) { try Command.parse(["schedule", "run"]) }
  }

  @Test func skipsWhileTheScreenIsLocked() throws {
    let project = try scratch()
    try saveProjectSchedules(project: project.path, schedules: [daily("Nightly", at: 9, 0)])
    var command = ScheduleCommand(project: project.path)
    command.progress = Console(write: { _ in })
    command.now = { Calendar.current.date(bySettingHour: 9, minute: 0, second: 0, of: .now)! }
    command.blocker = { .screenLocked }
    command.runTests = { _ throws(UsageError) in
      Issue.record("tests ran while the screen was locked")
      return []
    }
    #expect(command.execute() == .notReady)
    let runs = try scheduledRuns(project: project.path)
    #expect(runs.map(\.schedule) == ["Nightly"])
    #expect(runs.first?.outcome == .skipped(reason: .screenLocked))
    // Already handled: a second start at the same minute does nothing.
    #expect(command.execute() == .ok)
    #expect(try scheduledRuns(project: project.path).count == 1)
  }

  @Test func recordsWhyARunCouldNotStart() throws {
    let project = try scratch()
    try saveProjectSchedules(project: project.path, schedules: [daily("Nightly", at: 9, 0)])
    var command = ScheduleCommand(project: project.path)
    command.progress = Console(write: { _ in })
    command.now = { Calendar.current.date(bySettingHour: 9, minute: 0, second: 0, of: .now)! }
    command.blocker = { nil }
    command.runTests = { request throws(UsageError) in
      #expect(request.schedule == "Nightly")
      #expect(request.paths == [project.path])
      throw UsageError(message: "jevision: no tests")
    }
    #expect(command.execute() == .usage)
    #expect(try scheduledRuns(project: project.path).first?.outcome == .error(message: "no tests"))
  }

  @Test func aHistoryFromANewerJEVisionAsksForANewerOne() throws {
    let project = try scratch()
    try saveProjectSchedules(project: project.path, schedules: [daily("Nightly", at: 9, 0)])
    let history = try newerHistory(in: project)
    var command = ScheduleCommand(project: project.path)
    command.progress = Console(write: { _ in })
    command.now = { Calendar.current.date(bySettingHour: 9, minute: 0, second: 0, of: .now)! }
    command.blocker = { nil }
    command.runTests = { _ throws(UsageError) in
      Issue.record("tests ran though their history can't be kept")
      return []
    }
    #expect(command.execute() == .needsNewer)
    #expect(try String(contentsOf: history, encoding: .utf8) == Self.newer, "the newer history is left as it is")
  }

  static let newer = #"{"format": 99, "runs": []}"#
}

/// Saves a schedule history only a newer JEVision reads in `project`.
private func newerHistory(in project: URL) throws -> URL {
  let history = project.appending(path: ".jevision/schedule-history.json")
  try FileManager.default.createDirectory(at: history.deletingLastPathComponent(), withIntermediateDirectories: true)
  try ScheduleCommandTests.newer.write(to: history, atomically: true, encoding: .utf8)
  return history
}

@MainActor
struct ProjectSchedulesTests {
  @Test func aHistoryThatCantBeReadIsShownNotAnEmptyList() throws {
    let project = try scratch()
    try saveProjectSchedules(project: project.path, schedules: [daily("Nightly", at: 9, 0)])
    _ = try newerHistory(in: project)
    var agents = ScheduleAgents()
    agents.folder = try scratch().appending(path: "LaunchAgents")
    agents.launchctl = { _ in Issue.record("nothing is installed while the history can't be kept") }
    let schedules = ProjectSchedules(agents: agents, defaults: UserDefaults(suiteName: "ProjectSchedulesTests")!)
    schedules.open(project)
    #expect(schedules.all.map(\.name) == ["Nightly"], "the schedules themselves are read")
    #expect(schedules.problem?.contains(".jevision/schedule-history.json") == true, "\(schedules.problem ?? "no problem")")
  }
}

/// `launchctl` for the tests: keeps which jobs launchd has, without loading any. Like launchd, it
/// can still be taking a job out after `bootout` returns, and bootstrap fails while it has it.
private final class FakeLaunchd: @unchecked Sendable {
  var list: [[String]] = []
  /// How many more times `print` finds a job after `bootout`.
  var lingers = 0
  /// How many more times bootstrap fails, as when macOS doesn't allow the job.
  var refusals = 0
  /// launchctl was run on the main thread.
  var ranOnMain = false
  /// The jobs launchd has, with how many more times `print` finds each that is being taken out.
  private var jobs: [String: Int?] = [:]
  private let lock = NSLock()

  struct Failure: LocalizedError {
    var errorDescription: String? = "Could not find service"
  }

  func run(_ arguments: [String]) throws {
    try lock.withLock { try handle(arguments) }
  }

  private func handle(_ arguments: [String]) throws {
    list.append(arguments)
    ranOnMain = ranOnMain || Thread.isMainThread
    switch arguments[0] {
    case "bootstrap":
      let job = arguments[1] + "/" + URL(filePath: arguments[2]).deletingPathExtension().lastPathComponent
      guard refusals == 0, jobs[job] == nil else {
        refusals = max(0, refusals - 1)
        throw Failure(errorDescription: "Bootstrap failed: 5: Input/output error")
      }
      jobs[job] = .some(nil)
    case "bootout":
      guard jobs[arguments[1]] != nil else { throw Failure() }
      jobs[arguments[1]] = lingers
    case "print":
      guard let leaving = jobs[arguments[1]] else { throw Failure() }
      guard let leaving else { return }
      guard leaving > 0 else {
        jobs[arguments[1]] = nil
        throw Failure()
      }
      jobs[arguments[1]] = leaving - 1
    default: Issue.record("unexpected launchctl \(arguments)")
    }
  }
}

struct ScheduleAgentsTests {
  private func agents(in folder: URL, calls: FakeLaunchd, toolExists: Bool = true) throws -> ScheduleAgents {
    let tool = folder.appending(path: "jevision")
    if toolExists {
      try "#!/bin/sh\n".write(to: tool, atomically: true, encoding: .utf8)
      try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: tool.path)
    }
    var agents = ScheduleAgents()
    agents.folder = folder.appending(path: "LaunchAgents")
    agents.bundleID = "test.jevision"
    agents.tool = CommandLineTool(tool: tool)
    agents.launchctl = calls.run
    return agents
  }

  @Test func installsUpdatesAndRemovesTheJob() throws {
    let home = try scratch()
    let project = try scratch()
    let calls = FakeLaunchd()
    let agents = try agents(in: home, calls: calls)
    let schedules = [daily("Nightly", at: 3, 0)]
    #expect(agents.state(of: project, schedules: schedules) == (false, false))

    try agents.install(project, schedules: schedules)
    let file = agents.file(for: project)
    #expect(file.lastPathComponent.hasPrefix("test.jevision.schedule."))
    #expect(FileManager.default.fileExists(atPath: file.path))
    #expect(calls.list.map(\.first) == ["bootout", "print", "bootstrap"])
    #expect(calls.list.last?.last == file.path(percentEncoded: false))
    #expect(agents.state(of: project, schedules: schedules) == (true, true))
    #expect(agents.state(of: project, schedules: [daily("Nightly", at: 4, 0)]) == (true, false))
    #expect(agents.projects() == [ScheduleAgents.path(project)])

    calls.list = []
    try agents.install(project, schedules: [])
    #expect(!FileManager.default.fileExists(atPath: file.path))
    #expect(calls.list.map(\.first) == ["bootout"])
    #expect(agents.projects().isEmpty)
  }

  @Test func leavesTheJobAloneWhenLaunchdHasItAsItShouldBe() throws {
    let calls = FakeLaunchd()
    let agents = try agents(in: try scratch(), calls: calls)
    let project = try scratch()
    let schedules = [daily("Nightly", at: 3, 0)]
    try agents.install(project, schedules: schedules)
    calls.list = []

    try agents.install(project, schedules: schedules)
    #expect(calls.list.map(\.first) == ["print"])

    // Taken out of launchd some other way, the job is loaded again.
    try calls.run(["bootout", "gui/\(getuid())/\(agents.label(for: project))"])
    calls.list = []
    try agents.install(project, schedules: schedules)
    #expect(calls.list.map(\.first) == ["print", "bootout", "print", "bootstrap"])
  }

  @Test func replacingTheJobWaitsUntilLaunchdHasTakenTheOldOneOut() throws {
    let calls = FakeLaunchd()
    let agents = try agents(in: try scratch(), calls: calls)
    let project = try scratch()
    try agents.install(project, schedules: [daily("Nightly", at: 3, 0)])
    calls.lingers = 3
    calls.list = []

    try agents.install(project, schedules: [daily("Nightly", at: 4, 0)])
    #expect(calls.list.map(\.first) == ["bootout", "print", "print", "print", "print", "bootstrap"])
    #expect(agents.state(of: project, schedules: [daily("Nightly", at: 4, 0)]) == (true, true))
  }

  @Test func stopsWaitingForLaunchdAfterAWhile() throws {
    let calls = FakeLaunchd()
    var agents = try agents(in: try scratch(), calls: calls)
    agents.patience = .milliseconds(200)
    let project = try scratch()
    try agents.install(project, schedules: [daily("Nightly", at: 3, 0)])
    calls.lingers = .max

    let started = ContinuousClock.now
    #expect(throws: ScheduleAgents.Failure.self) {
      try agents.install(project, schedules: [daily("Nightly", at: 4, 0)])
    }
    #expect(ContinuousClock.now - started < .seconds(5))
    #expect(calls.list.last?.first == "print")
    #expect(agents.state(of: project, schedules: [daily("Nightly", at: 4, 0)]) == (false, false))
  }

  @Test func refusesWithoutTheTool() throws {
    let home = try scratch()
    let calls = FakeLaunchd()
    let agents = try agents(in: home, calls: calls, toolExists: false)
    #expect(throws: (any Error).self) { try agents.install(try scratch(), schedules: [daily("Nightly", at: 3, 0)]) }
    #expect(calls.list.isEmpty)
  }
}

@MainActor
struct ScheduleNotifierTests {
  @Test func tellsOnlyAboutNewRuns() throws {
    let project = try scratch().path
    let suite = "jevision-schedule-tests-\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    defer { defaults.removePersistentDomain(forName: suite) }
    func record(_ ms: Int64, failed: UInt32) throws {
      try recordScheduledRun(
        project: project,
        run: ScheduledRun(schedule: "Nightly", startedAtMs: ms, durationMs: 1, outcome: .finished(passed: 1, failed: failed)))
    }
    try record(1_000, failed: 1)
    // A project seen for the first time doesn't bring up its old runs.
    #expect(ScheduleNotifier.unseen(project, defaults: defaults).isEmpty)
    try record(2_000, failed: 0)
    try record(3_000, failed: 2)
    let new = ScheduleNotifier.unseen(project, defaults: defaults)
    #expect(new.map(\.startedAtMs) == [2_000, 3_000])
    #expect(new.map(\.passed) == [true, false])
    #expect(ScheduleNotifier.unseen(project, defaults: defaults).isEmpty)
  }
}

struct ScheduleNameTests {
  @Test func picksTheFirstFreeName() {
    #expect(SchedulesSheet.freeName("Nightly", taken: []) == "Nightly")
    #expect(SchedulesSheet.freeName("Nightly", taken: ["nightly"]) == "Nightly 2")
    #expect(SchedulesSheet.freeName("Nightly", taken: ["Nightly", "Nightly 2", "Hourly"]) == "Nightly 3")
  }
}

@MainActor
struct ScheduleUndoTests {
  private let never: FileUndo.Ask = { _, _ in
    Issue.record("asked to replace the file")
    return false
  }

  /// Schedules of a new project, with macOS's job kept in a scratch folder.
  private func schedules(
    _ project: URL, launchd: FakeLaunchd = FakeLaunchd(), patience: Duration = .seconds(3)
  ) throws -> (ProjectSchedules, ScheduleAgents) {
    let home = try scratch()
    let tool = home.appending(path: "jevision")
    try "#!/bin/sh\n".write(to: tool, atomically: true, encoding: .utf8)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: tool.path)
    var agents = ScheduleAgents()
    agents.folder = home.appending(path: "LaunchAgents")
    agents.bundleID = "test.jevision"
    agents.tool = CommandLineTool(tool: tool)
    agents.launchctl = launchd.run
    agents.patience = patience
    let suite = "jevision-schedule-undo-\(UUID().uuidString)"
    let schedules = ProjectSchedules(agents: agents, defaults: try #require(UserDefaults(suiteName: suite)))
    schedules.open(project)
    return (schedules, agents)
  }

  private func undoManager() -> UndoManager {
    let undo = UndoManager()
    undo.groupsByEvent = false
    return undo
  }

  /// Saves `list` as one step of `undo` named `action`; what macOS made of it is left to await.
  private func save(
    _ list: [ProjectSchedule], in schedules: ProjectSchedules, _ action: String, _ undo: UndoManager,
    ask: FileUndo.Ask? = nil
  ) throws -> Task<Void, any Error> {
    undo.beginUndoGrouping()
    defer { undo.endUndoGrouping() }
    return try schedules.save(list, action: action, undoManager: undo, ask: ask ?? never)
  }

  /// Waits until macOS has taken up every change asked of it, for no longer than `limit`.
  private func settled(_ schedules: ProjectSchedules, within limit: Duration = .seconds(10)) async throws {
    let deadline = ContinuousClock.now + limit
    while schedules.isChanging {
      try #require(ContinuousClock.now < deadline, "macOS is still being asked")
      try await Task.sleep(for: .milliseconds(5))
    }
  }

  @Test func undoAndRedoPutTheFileBackAndMacOSFollows() async throws {
    let project = try scratch()
    let (schedules, agents) = try schedules(project)
    let file = project.appending(path: scheduleFileName())
    let job = agents.file(for: project)
    let undo = undoManager()

    try await save([daily("Nightly", at: 3, 0)], in: schedules, "Add Schedule", undo).value
    try await save([], in: schedules, "Delete Schedule", undo).value
    #expect(undo.undoActionName == "Delete Schedule")
    #expect(!FileManager.default.fileExists(atPath: file.path))
    #expect(!FileManager.default.fileExists(atPath: job.path))

    undo.undo()
    try await settled(schedules)
    #expect(schedules.all.map(\.name) == ["Nightly"])
    #expect(FileManager.default.fileExists(atPath: job.path))
    #expect(schedules.isOn)
    undo.undo()
    try await settled(schedules)
    #expect(!FileManager.default.fileExists(atPath: file.path))
    #expect(schedules.all.isEmpty)
    #expect(!FileManager.default.fileExists(atPath: job.path))
    undo.redo()
    try await settled(schedules)
    #expect(try projectSchedules(project: project.path).map(\.name) == ["Nightly"])
    #expect(schedules.all.map(\.name) == ["Nightly"])
    #expect(schedules.isOn)
  }

  @Test func undoReplacesTheJobOnceAndOnlyWhenTheTimesChange() async throws {
    let project = try scratch()
    let launchd = FakeLaunchd()
    let (schedules, _) = try schedules(project, launchd: launchd)
    let undo = undoManager()
    try await save([daily("Nightly", at: 3, 0)], in: schedules, "Add Schedule", undo).value
    try await save([daily("Nightly", at: 4, 0)], in: schedules, "Edit Schedule", undo).value
    launchd.list = []

    undo.undo()
    try await settled(schedules)
    #expect(launchd.list.map(\.first) == ["bootout", "print", "bootstrap"])
    #expect(schedules.isOn)

    // Another environment, at the same time: the job stays as it is, and so would a run in progress.
    launchd.list = []
    let staging = ProjectSchedule(
      name: "Nightly", tests: .all, repeats: .daily, hour: 3, minute: 0, environment: "Staging", video: nil)
    try await save([staging], in: schedules, "Edit Schedule", undo).value
    undo.undo()
    try await settled(schedules)
    undo.redo()
    try await settled(schedules)
    #expect(launchd.list.map(\.first) == ["print"])
    #expect(schedules.all.map(\.environment) == ["Staging"])
    #expect(schedules.isOn)
  }

  @Test func launchdIsAskedAwayFromTheMainThreadOneChangeAtATime() async throws {
    let project = try scratch()
    let launchd = FakeLaunchd()
    let (schedules, _) = try schedules(project, launchd: launchd)
    let undo = undoManager()
    try await save([daily("Nightly", at: 3, 0)], in: schedules, "Add Schedule", undo).value
    launchd.lingers = 3
    launchd.list = []

    let first = try save([daily("Nightly", at: 4, 0)], in: schedules, "Edit Schedule", undo)
    #expect(schedules.isChanging)
    let second = try save([daily("Nightly", at: 5, 0)], in: schedules, "Edit Schedule", undo)
    try await settled(schedules)
    try await first.value
    try await second.value
    let replace = ["bootout", "print", "print", "print", "print", "bootstrap"]
    #expect(launchd.list.map(\.first) == replace + replace)
    #expect(!launchd.ranOnMain)
    #expect(!schedules.isChanging)
    #expect(schedules.isOn)
  }

  @Test func savedSchedulesStaySavedWhenMacOSDoesNotTakeThem() async throws {
    let project = try scratch()
    let launchd = FakeLaunchd()
    launchd.refusals = .max
    let (schedules, agents) = try schedules(project, launchd: launchd)
    let undo = undoManager()

    let turningOn = try save([daily("Nightly", at: 3, 0)], in: schedules, "Add Schedule", undo)
    #expect(try projectSchedules(project: project.path).map(\.name) == ["Nightly"])
    #expect(schedules.all.map(\.name) == ["Nightly"])
    #expect(undo.undoActionName == "Add Schedule")
    let failure = await #expect(throws: ScheduleAgents.Failure.self) { try await turningOn.value }
    #expect(failure?.errorDescription?.contains("Login Items") == true)
    #expect(failure?.detail == "Bootstrap failed: 5: Input/output error")
    #expect(!schedules.isOn)
    #expect(!FileManager.default.fileExists(atPath: agents.file(for: project).path))

    launchd.refusals = 0
    try await schedules.turnOn()
    #expect(schedules.isOn)
  }

  @Test func aSaveThatFailedIsNotRetriedOnItsOwn() async throws {
    let project = try scratch()
    let launchd = FakeLaunchd()
    let (schedules, agents) = try schedules(project, launchd: launchd, patience: .milliseconds(200))
    let undo = undoManager()
    try await save([daily("Nightly", at: 3, 0)], in: schedules, "Add Schedule", undo).value
    launchd.lingers = .max

    let turningOn = try save([daily("Nightly", at: 4, 0)], in: schedules, "Edit Schedule", undo)
    try await settled(schedules)
    await #expect(throws: ScheduleAgents.Failure.self) { try await turningOn.value }
    #expect(!FileManager.default.fileExists(atPath: agents.file(for: project).path))
    launchd.list = []
    schedules.reload()
    #expect(!schedules.isChanging)
    #expect(launchd.list.isEmpty)
    #expect(!schedules.isOn)
  }

  @Test func savingSaysItFailedOnlyWhenTheSchedulesEndUpNotRunning() async throws {
    let project = try scratch()
    let launchd = FakeLaunchd()
    launchd.refusals = 1
    let (schedules, _) = try schedules(project, launchd: launchd)
    let undo = undoManager()

    let first = try save([daily("Nightly", at: 3, 0)], in: schedules, "Add Schedule", undo)
    let again = try save([daily("Nightly", at: 3, 0)], in: schedules, "Add Schedule", undo)
    try await first.value
    try await again.value
    #expect(launchd.list.filter { $0.first == "bootstrap" }.count == 2)
    #expect(schedules.isOn)
  }

  @Test func askDecidesWhenTheFileChangedSomewhereElse() async throws {
    let project = try scratch()
    let (schedules, _) = try schedules(project)
    let undo = undoManager()
    var replaces = false
    var asked = 0
    let ask: FileUndo.Ask = { _, _ in
      asked += 1
      return replaces
    }
    try await save([daily("Nightly", at: 3, 0)], in: schedules, "Add Schedule", undo, ask: ask).value
    try saveProjectSchedules(project: project.path, schedules: [daily("Hourly", at: 4, 0)])

    undo.undo()
    #expect(asked == 1)
    #expect(try projectSchedules(project: project.path).map(\.name) == ["Hourly"])

    try await save([daily("Nightly", at: 3, 0)], in: schedules, "Edit Schedule", undo, ask: ask).value
    try saveProjectSchedules(project: project.path, schedules: [daily("Weekly", at: 5, 0)])
    replaces = true
    undo.undo()
    try await settled(schedules)
    #expect(asked == 2)
    #expect(try projectSchedules(project: project.path).map(\.name) == ["Hourly"])
    #expect(schedules.all.map(\.name) == ["Hourly"])
  }
}

struct ScheduleConflictTests {
  @Test func tellsWhatChangedInTheFileSinceEditingBegan() {
    let nightly = daily("Nightly", at: 3, 0)
    let draft = ScheduleDraft(nightly)
    let edited = daily("Nightly", at: 4, 0)
    #expect(draft.conflict(saving: edited, in: [nightly]) == nil)
    #expect(draft.conflict(saving: daily("nightly", at: 3, 0), in: [nightly]) == nil)
    #expect(draft.conflict(saving: edited, in: [daily("Nightly", at: 5, 0)]) == .changed)
    #expect(draft.conflict(saving: edited, in: [daily("Hourly", at: 5, 0)]) == .removed)
    #expect(draft.conflict(saving: daily("Hourly", at: 4, 0), in: [nightly, daily("hourly", at: 5, 0)]) == .nameTaken)

    let new = ScheduleDraft(name: "Hourly", environment: nil)
    #expect(new.conflict(saving: daily("Hourly", at: 4, 0), in: [nightly]) == nil)
    #expect(new.conflict(saving: daily("Hourly", at: 4, 0), in: [daily("HOURLY", at: 5, 0)]) == .nameTaken)
  }
}
