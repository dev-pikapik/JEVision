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

  @Test func incompatiblePreferencesStopBeforeScheduleCheckpointOrReadiness() throws {
    let project = try scratch()
    try saveProjectSchedules(project: project.path, schedules: [daily("Nightly", at: 9, 0)])
    let profile = Profile(folder: try scratch())
    defer { profile.defaults.removePersistentDomain(forName: profile.defaultsDomain) }
    profile.defaults.set(99, forKey: AppPreferences.formatKey)
    profile.defaults.synchronize()
    var command = ScheduleCommand(project: project.path)
    command.checkPreferences = { try AppPreferences.check(profile.defaults) }
    command.blocker = { Issue.record("Readiness was checked before preference compatibility"); return nil }
    command.runTests = { _ throws(UsageError) in Issue.record("Tests must not start"); return [] }
    #expect(command.execute() == .needsNewer)
    #expect(!FileManager.default.fileExists(atPath: project.appending(path: ".jevision/schedule-history.json").path))
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

  @Test func newerScheduleRequirementsStopBeforeCheckingTheScreen() throws {
    let project = try scratch()
    let file = project.appending(path: scheduleFileName())
    for text in [#"{"format":99,"schedules":false}"#, #"{"format":1,"minVersion":"9999.0.0","schedules":false}"#] {
      try text.write(to: file, atomically: true, encoding: .utf8)
      var command = ScheduleCommand(project: project.path)
      command.blocker = { Testing.Issue.record("readiness was checked for an unsupported file"); return .screenLocked }
      command.runTests = { _ throws(UsageError) in Testing.Issue.record("unsupported schedules ran"); return [] }
      #expect(command.execute() == .needsNewer)
      #expect(try String(contentsOf: file, encoding: .utf8) == text)
      #expect(!FileManager.default.fileExists(atPath: project.appending(path: ".jevision/schedule-history.json").path))
    }
  }

  static let newer = #"{"format": 99, "runs": []}"#

  @Test func unsupportedOrBrokenHistoryStopsBeforeScreenChecksAndInput() throws {
    let project = try scratch()
    defer { try? FileManager.default.removeItem(at: project) }
    try saveProjectSchedules(project: project.path, schedules: [daily("Nightly", at: 9, 0)])
    let history = try newerHistory(in: project)
    for (text, expected) in [
      (#"{"format":1,"minVersion":"9999.0.0","runs":false}"#, ExitStatus.needsNewer),
      (#"{"format":1,"minVersion":"invalid","runs":[]}"#, .io),
      (#"{"format":1,"checkedAt":"broken","runs":[]}"#, .io),
      (#"{"format":1,"runs":[],"runs":[]}"#, .io),
      ("{", .io),
    ] {
      try text.write(to: history, atomically: true, encoding: .utf8)
      var command = ScheduleCommand(project: project.path)
      command.blocker = { Testing.Issue.record("screen readiness was checked despite unreadable history"); return .screenLocked }
      command.runTests = { _ throws(UsageError) in Testing.Issue.record("a test ran despite unreadable history"); return [] }
      #expect(command.execute() == expected)
      #expect(try String(contentsOf: history, encoding: .utf8) == text)
      #expect(!FileManager.default.fileExists(atPath: project.appending(path: ".jevision/runs").path))
    }
  }

  @Test func checkpointAndSkippedRunPreserveHistoryMetadata() throws {
    let project = try scratch()
    defer { try? FileManager.default.removeItem(at: project) }
    try saveProjectSchedules(project: project.path, schedules: [daily("Nightly", at: 9, 0)])
    let history = try newerHistory(in: project)
    let date = try #require(Calendar.current.date(bySettingHour: 9, minute: 0, second: 0, of: .now))
    let row: [String: Any] = ["schedule": "Previous", "startedAtMs": 0, "durationMs": 0,
      "host": ["id": 42], "outcome": ["result": "finished", "passed": 1, "failed": 0, "report": "retained"]]
    let original: [String: Any] = ["format": 1, "minVersion": "0.1.0", "checkedAt": LocalMinute.of(date) - 1,
      "runs": [row], "cursor": ["generation": 3]]
    try JSONSerialization.data(withJSONObject: original).write(to: history)
    var command = ScheduleCommand(project: project.path)
    command.progress = Console(write: { _ in })
    command.now = { date }
    command.blocker = { .screenLocked }
    command.runTests = { _ throws(UsageError) in Testing.Issue.record("tests ran while blocked"); return [] }
    #expect(command.execute() == .notReady)
    let saved = try #require(JSONSerialization.jsonObject(with: Data(contentsOf: history)) as? [String: Any])
    let rows = try #require(saved["runs"] as? [[String: Any]])
    #expect(rows.count == 2)
    #expect(NSDictionary(dictionary: rows[0]) == NSDictionary(dictionary: row))
    #expect((saved["cursor"] as? [String: Int]) == ["generation": 3])
    #expect(saved["minVersion"] as? String == "0.1.0")
    #expect(try scheduledRuns(project: project.path).first?.outcome == .skipped(reason: .screenLocked))
  }
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
    agents.isAvailable = true
    agents.checkPreferences = {}
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
    #expect(calls.list.map(\.first) == ["bootout", "print"])
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

  @Test func aJobFileAloneDoesNotMeanLaunchdHasTheJob() throws {
    let calls = FakeLaunchd()
    let agents = try agents(in: try scratch(), calls: calls)
    let project = try scratch()
    let schedules = [daily("Nightly", at: 3, 0)]
    try agents.install(project, schedules: schedules)
    try calls.run(["bootout", "gui/\(getuid())/\(agents.label(for: project))"])
    calls.list = []
    #expect(agents.state(of: project, schedules: schedules) == (true, false))
    #expect(calls.list.map(\.first) == ["print"])
    #expect(FileManager.default.fileExists(atPath: agents.file(for: project).path))
  }

  @Test func startupReconciliationKeepsTheLiveJobAndRestoresOnlyPreviouslyEnabledProjects() throws {
    let calls = FakeLaunchd()
    let agents = try agents(in: try scratch(), calls: calls)
    let project = try scratch()
    let disabled = try scratch()
    let schedules = [daily("Nightly", at: 3, 0)]
    for folder in [project, disabled] { try saveProjectSchedules(project: folder.path, schedules: schedules) }
    try agents.install(project, schedules: schedules)
    calls.list = []
    #expect(agents.reconcile().isEmpty)
    #expect(calls.list.map(\.first) == ["print"], "an active unchanged run is never stopped")
    #expect(!FileManager.default.fileExists(atPath: agents.file(for: disabled).path))

    try calls.run(["bootout", "gui/\(getuid())/\(agents.label(for: project))"])
    calls.list = []
    #expect(agents.reconcile().isEmpty)
    #expect(calls.list.map(\.first) == ["print", "bootout", "print", "bootstrap"])
    #expect(agents.state(of: project, schedules: schedules) == (true, true))
    #expect(!FileManager.default.fileExists(atPath: agents.file(for: disabled).path))
  }

  @Test func startupReconciliationUpdatesTheToolPathAfterMovingTheApp() throws {
    let calls = FakeLaunchd()
    let original = try agents(in: try scratch(), calls: calls)
    let project = try scratch()
    let schedules = [daily("Nightly", at: 3, 0)]
    try saveProjectSchedules(project: project.path, schedules: schedules)
    try original.install(project, schedules: schedules)
    let movedTool = try scratch().appending(path: "jevision")
    try "#!/bin/sh\n".write(to: movedTool, atomically: true, encoding: .utf8)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: movedTool.path)
    var moved = original
    moved.tool = CommandLineTool(tool: movedTool)
    calls.list = []

    #expect(moved.reconcile().isEmpty)
    #expect(calls.list.map(\.first) == ["bootout", "print", "bootstrap"])
    #expect(moved.state(of: project, schedules: schedules) == (true, true))
    #expect(original.state(of: project, schedules: schedules) == (true, false))
  }

  @Test func filesWithAnotherIdentityOrCommandAreNeverAdoptedOrReplaced() throws {
    let mutations: [(inout [String: Any], String) -> Void] = [
      { plist, _ in plist["Label"] = "test.jevision.schedule.another" },
      { plist, _ in plist["AssociatedBundleIdentifiers"] = ["another.app"] },
      { plist, path in plist["ProgramArguments"] = ["/bin/echo", "schedule", "run", path] },
      { plist, path in plist["ProgramArguments"] = ["jevision", "schedule", "run", path] },
      { plist, path in plist["ProgramArguments"] = ["/tmp/jevision", "run", "schedule", path] },
      { plist, _ in plist["ProgramArguments"] = ["/tmp/jevision", "schedule", "run", "/another/project"] },
      { plist, path in plist["ProgramArguments"] = ["/tmp/jevision", "schedule", "run", path + "/../other"] },
    ]
    for mutate in mutations {
      let calls = FakeLaunchd()
      let agents = try agents(in: try scratch(), calls: calls)
      let project = try scratch()
      let schedules = [daily("Nightly", at: 3, 0)]
      try agents.install(project, schedules: schedules)
      let file = agents.file(for: project)
      var plist = try #require(PropertyListSerialization.propertyList(from: Data(contentsOf: file), format: nil) as? [String: Any])
      mutate(&plist, project.path)
      let bytes = try PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0)
      try bytes.write(to: file)
      calls.list = []

      #expect(agents.projects().isEmpty)
      #expect(agents.reconcile().isEmpty)
      #expect(agents.state(of: project, schedules: schedules) == (false, false))
      #expect(throws: CocoaError.self) { try agents.install(project, schedules: schedules) }
      #expect(throws: CocoaError.self) { try agents.remove(project) }
      #expect(calls.list.isEmpty)
      #expect(try Data(contentsOf: file) == bytes)
    }
  }

  @Test func anotherFilenameOrASymlinkIsNotARecordOfConsent() throws {
    let calls = FakeLaunchd()
    let agents = try agents(in: try scratch(), calls: calls)
    let project = try scratch()
    let schedules = [daily("Nightly", at: 3, 0)]
    try agents.install(project, schedules: schedules)
    let file = agents.file(for: project)
    let renamed = file.deletingLastPathComponent().appending(path: "test.jevision.schedule.another.plist")
    try FileManager.default.moveItem(at: file, to: renamed)
    calls.list = []
    #expect(agents.projects().isEmpty)
    #expect(agents.reconcile().isEmpty)
    let bytes = try Data(contentsOf: renamed)
    try FileManager.default.createSymbolicLink(at: file, withDestinationURL: renamed)
    #expect(agents.projects().isEmpty)
    #expect(agents.reconcile().isEmpty)
    #expect(throws: CocoaError.self) { try agents.install(project, schedules: schedules) }
    #expect(throws: CocoaError.self) { try agents.remove(project) }
    #expect(try Data(contentsOf: renamed) == bytes)
    #expect((try file.resourceValues(forKeys: [.isSymbolicLinkKey])).isSymbolicLink == true)
    #expect(calls.list.isEmpty)
  }

  @Test func unreadableRequirementsOrHistoryKeepThePreviouslyEnabledJobIntact() throws {
    for history in [false, true] {
      for text in ["{", #"{"format":99,"schedules":false,"runs":false}"#] {
        let calls = FakeLaunchd()
        let agents = try agents(in: try scratch(), calls: calls)
        let project = try scratch()
        let schedules = [daily("Nightly", at: 3, 0)]
        try saveProjectSchedules(project: project.path, schedules: schedules)
        try agents.install(project, schedules: schedules)
        let file = agents.file(for: project)
        let bytes = try Data(contentsOf: file)
        let changed = history ? project.appending(path: ".jevision/schedule-history.json") : project.appending(path: scheduleFileName())
        try FileManager.default.createDirectory(at: changed.deletingLastPathComponent(), withIntermediateDirectories: true)
        try text.write(to: changed, atomically: true, encoding: .utf8)
        calls.list = []

        #expect(agents.reconcile().map(\.project) == [ScheduleAgents.path(project)])
        #expect(calls.list.isEmpty)
        #expect(try Data(contentsOf: file) == bytes)
        #expect(try String(contentsOf: changed, encoding: .utf8) == text)
      }
    }
  }

  @Test func aMissingProjectDoesNotStopReconciliationOfTheOthers() throws {
    let calls = FakeLaunchd()
    let agents = try agents(in: try scratch(), calls: calls)
    let missing = try scratch()
    let available = try scratch()
    let schedules = [daily("Nightly", at: 3, 0)]
    for project in [missing, available] {
      try saveProjectSchedules(project: project.path, schedules: schedules)
      try agents.install(project, schedules: schedules)
    }
    let missingFile = agents.file(for: missing)
    let missingPath = ScheduleAgents.path(missing)
    let preserved = try Data(contentsOf: missingFile)
    try FileManager.default.removeItem(at: missing)
    try calls.run(["bootout", "gui/\(getuid())/\(agents.label(for: available))"])
    calls.list = []
    #expect(agents.reconcile().map(\.project) == [missingPath])
    #expect(try Data(contentsOf: missingFile) == preserved)
    #expect(calls.list.filter { $0.first == "bootstrap" }.count == 1)
    #expect(agents.state(of: available, schedules: schedules) == (true, true))
  }

  @Test func newerPreferencesRefuseReconciliationAndEveryAgentMutation() throws {
    let calls = FakeLaunchd()
    var agents = try agents(in: try scratch(), calls: calls)
    let project = try scratch()
    let schedules = [daily("Nightly", at: 3, 0)]
    try saveProjectSchedules(project: project.path, schedules: schedules)
    try agents.install(project, schedules: schedules)
    let file = agents.file(for: project)
    let bytes = try Data(contentsOf: file)
    let profile = Profile(folder: try scratch())
    defer { profile.defaults.removePersistentDomain(forName: profile.defaultsDomain) }
    profile.defaults.set(99, forKey: AppPreferences.formatKey)
    profile.defaults.synchronize()
    agents.checkPreferences = { try AppPreferences.check(profile.defaults) }
    calls.list = []
    #expect(throws: AppPreferences.Failure.self) { try agents.install(project, schedules: schedules) }
    #expect(throws: AppPreferences.Failure.self) { try agents.remove(project) }
    #expect(agents.reconcile().isEmpty)
    #expect(agents.state(of: project, schedules: schedules) == (false, false))
    #expect(calls.list.isEmpty)
    #expect(try Data(contentsOf: file) == bytes)
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
    #expect(agents.state(of: project, schedules: [daily("Nightly", at: 4, 0)]) == (true, false))
  }

  @Test func removingAJobWaitsForLaunchdAndPreservesTheFileWhenItCannotStop() throws {
    let calls = FakeLaunchd()
    var agents = try agents(in: try scratch(), calls: calls)
    agents.patience = .milliseconds(100)
    let project = try scratch()
    try agents.install(project, schedules: [daily("Nightly", at: 3, 0)])
    let file = agents.file(for: project)
    let bytes = try Data(contentsOf: file)
    calls.lingers = .max
    calls.list = []
    #expect(throws: ScheduleAgents.Failure.self) { try agents.remove(project) }
    #expect(calls.list.first?.first == "bootout")
    #expect(calls.list.last?.first == "print")
    #expect(try Data(contentsOf: file) == bytes)
    calls.lingers = 0
    calls.list = []
    try agents.remove(project)
    #expect(calls.list.map(\.first) == ["bootout", "print"])
    #expect(!FileManager.default.fileExists(atPath: file.path))
  }

  @Test func refusesWithoutTheTool() throws {
    let home = try scratch()
    let calls = FakeLaunchd()
    let agents = try agents(in: home, calls: calls, toolExists: false)
    #expect(throws: (any Error).self) { try agents.install(try scratch(), schedules: [daily("Nightly", at: 3, 0)]) }
    #expect(calls.list.isEmpty)
  }

  @Test func aSeparateProfileLeavesThePersonsJobsAlone() throws {
    let home = try scratch()
    let project = try scratch()
    let calls = FakeLaunchd()
    var agents = try agents(in: home, calls: calls)
    let schedules = [daily("Nightly", at: 3, 0)]
    try agents.install(project, schedules: schedules)
    calls.list = []

    agents.isAvailable = false
    #expect(agents.state(of: project, schedules: schedules) == (false, false))
    #expect(agents.projects().isEmpty)
    #expect(agents.reconcile().isEmpty)
    #expect(throws: (any Error).self) { try agents.install(project, schedules: [daily("Early", at: 5, 0)]) }
    try agents.remove(project)
    #expect(calls.list.isEmpty)
    #expect(FileManager.default.fileExists(atPath: agents.file(for: project).path))
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
    agents.isAvailable = true
    agents.checkPreferences = {}
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

  @Test func renamedScheduleMetadataSurvivesUndoAndRedo() async throws {
    let project = try scratch()
    let file = project.appending(path: scheduleFileName())
    let original = #"{"format":1,"minVersion":"1.0.0","future":true,"schedules":[{"name":"Nightly","tests":"all","repeat":"daily","at":"09:00","optional":{"keep":42}}]}"#
    try original.write(to: file, atomically: true, encoding: .utf8)
    let (store, _) = try schedules(project)
    #expect(store.warnings.count == 2)
    let undo = undoManager()
    undo.beginUndoGrouping()
    let task = try store.save([daily("Evening", at: 18, 0)], renaming: ("Nightly", "Evening"), action: "Rename", undoManager: undo, ask: never)
    undo.endUndoGrouping()
    try await task.value
    func value() throws -> [String: Any] {
      try #require(JSONSerialization.jsonObject(with: Data(contentsOf: file)) as? [String: Any])
    }
    let entries = try #require(try value()["schedules"] as? [[String: Any]])
    #expect(entries.first?["name"] as? String == "Evening")
    #expect((entries.first?["optional"] as? [String: Int])?["keep"] == 42)
    undo.undo()
    try await settled(store)
    #expect(try String(contentsOf: file, encoding: .utf8) == original)
    undo.redo()
    try await settled(store)
    let redone = try #require(try value()["schedules"] as? [[String: Any]])
    #expect(redone.first?["name"] as? String == "Evening")
    #expect((redone.first?["optional"] as? [String: Int])?["keep"] == 42)
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
    #expect(launchd.list.map(\.first) == ["print", "print", "print"])
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

  @Test func reloadRechecksLaunchdAndRestoresAPreviouslyEnabledJobOffTheMainThread() async throws {
    let project = try scratch()
    let launchd = FakeLaunchd()
    let (schedules, agents) = try schedules(project, launchd: launchd)
    try await schedules.save([daily("Nightly", at: 3, 0)], action: "Add Schedule", undoManager: nil).value
    try launchd.run(["bootout", "gui/\(getuid())/\(agents.label(for: project))"])
    launchd.list = []
    launchd.ranOnMain = false

    schedules.reload()
    try await settled(schedules)
    #expect(schedules.isOn)
    #expect(launchd.list.map(\.first) == ["print", "bootout", "print", "bootstrap"])
    #expect(!launchd.ranOnMain)

    launchd.list = []
    schedules.reload()
    try await settled(schedules)
    #expect(schedules.isOn)
    #expect(launchd.list.map(\.first) == ["print"])
  }

  @Test func openingAProjectWithSchedulesDoesNotEnableItOnThisMac() throws {
    let project = try scratch()
    let launchd = FakeLaunchd()
    try saveProjectSchedules(project: project.path, schedules: [daily("Nightly", at: 3, 0)])
    let (schedules, agents) = try schedules(project, launchd: launchd)
    schedules.reload()
    #expect(!schedules.isOn)
    #expect(!schedules.isChanging)
    #expect(launchd.list.isEmpty)
    #expect(!FileManager.default.fileExists(atPath: agents.file(for: project).path))
  }

  @Test func finishingAnotherProjectsChangeDoesNotMarkTheNewProjectAsEnabled() async throws {
    let original = try scratch()
    let other = try scratch()
    let launchd = FakeLaunchd()
    let (schedules, agents) = try schedules(original, launchd: launchd)
    try await schedules.save([daily("Nightly", at: 3, 0)], action: "Add Schedule", undoManager: nil).value
    let changed = [daily("Nightly", at: 4, 0)]
    try saveProjectSchedules(project: other.path, schedules: changed)
    let saving = try schedules.save(changed, action: "Edit Schedule", undoManager: nil)
    schedules.open(other)
    try await saving.value
    try await settled(schedules)
    #expect(schedules.all == changed)
    #expect(!schedules.isOn)
    #expect(!FileManager.default.fileExists(atPath: agents.file(for: other).path))
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

  @Test func aSaveThatFailedPreservesThePreviousJobWithoutRetryingItself() async throws {
    let project = try scratch()
    let launchd = FakeLaunchd()
    let (schedules, agents) = try schedules(project, launchd: launchd, patience: .milliseconds(200))
    let undo = undoManager()
    try await save([daily("Nightly", at: 3, 0)], in: schedules, "Add Schedule", undo).value
    launchd.lingers = .max

    let turningOn = try save([daily("Nightly", at: 4, 0)], in: schedules, "Edit Schedule", undo)
    try await settled(schedules)
    await #expect(throws: ScheduleAgents.Failure.self) { try await turningOn.value }
    let previous = try #require(try agents.agent(for: project, schedules: [daily("Nightly", at: 3, 0)]))
    #expect(try String(contentsOf: agents.file(for: project), encoding: .utf8) == previous.plist)
    launchd.list = []
    try await Task.sleep(for: .milliseconds(50))
    #expect(!schedules.isChanging)
    #expect(launchd.list.isEmpty, "a failed change never retries itself")
    #expect(!schedules.isOn)
    launchd.lingers = 0
    schedules.reload()
    try await settled(schedules)
    #expect(schedules.isOn, "an explicit refresh can recover after macOS is ready")
  }

  @Test func failureToStopTheLastScheduleIsReportedRatherThanPretendingItStopped() async throws {
    let project = try scratch()
    let launchd = FakeLaunchd()
    let (schedules, agents) = try schedules(project, launchd: launchd, patience: .milliseconds(100))
    try await schedules.save([daily("Nightly", at: 3, 0)], action: "Add Schedule", undoManager: nil).value
    launchd.lingers = .max
    let clearing = try schedules.save([], action: "Delete Schedule", undoManager: nil)
    await #expect(throws: ScheduleAgents.Failure.self) { try await clearing.value }
    #expect(schedules.all.isEmpty)
    #expect(!schedules.isOn)
    #expect(FileManager.default.fileExists(atPath: agents.file(for: project).path))
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
    @MainActor final class Decision {
      var replaces = false
      var asked = 0
    }
    let decision = Decision()
    let ask: FileUndo.Ask = { _, _ in
      decision.asked += 1
      return decision.replaces
    }
    try await save([daily("Nightly", at: 3, 0)], in: schedules, "Add Schedule", undo, ask: ask).value
    try saveProjectSchedules(project: project.path, schedules: [daily("Hourly", at: 4, 0)])

    undo.undo()
    #expect(decision.asked == 1)
    #expect(try projectSchedules(project: project.path).map(\.name) == ["Hourly"])

    try await save([daily("Nightly", at: 3, 0)], in: schedules, "Edit Schedule", undo, ask: ask).value
    try saveProjectSchedules(project: project.path, schedules: [daily("Weekly", at: 5, 0)])
    decision.replaces = true
    undo.undo()
    try await settled(schedules)
    #expect(decision.asked == 2)
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
