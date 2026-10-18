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
    #expect(command.execute() == .problems)
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
    #expect(command.execute() == .failure)
    #expect(try scheduledRuns(project: project.path).first?.outcome == .error(message: "no tests"))
  }
}

struct ScheduleAgentsTests {
  private final class Calls: @unchecked Sendable { var list: [[String]] = [] }

  private func agents(in folder: URL, calls: Calls, toolExists: Bool = true) throws -> ScheduleAgents {
    let tool = folder.appending(path: "jevision")
    if toolExists {
      try "#!/bin/sh\n".write(to: tool, atomically: true, encoding: .utf8)
      try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: tool.path)
    }
    var agents = ScheduleAgents()
    agents.folder = folder.appending(path: "LaunchAgents")
    agents.bundleID = "test.jevision"
    agents.tool = CommandLineTool(tool: tool)
    agents.launchctl = { calls.list.append($0) }
    return agents
  }

  @Test func installsUpdatesAndRemovesTheJob() throws {
    let home = try scratch()
    let project = try scratch()
    let calls = Calls()
    let agents = try agents(in: home, calls: calls)
    let schedules = [daily("Nightly", at: 3, 0)]
    #expect(agents.state(of: project, schedules: schedules) == (false, false))

    try agents.install(project, schedules: schedules)
    let file = agents.file(for: project)
    #expect(file.lastPathComponent.hasPrefix("test.jevision.schedule."))
    #expect(FileManager.default.fileExists(atPath: file.path))
    #expect(calls.list.map(\.first) == ["bootout", "bootstrap"])
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

  @Test func refusesWithoutTheTool() throws {
    let home = try scratch()
    let calls = Calls()
    let agents = try agents(in: home, calls: calls, toolExists: false)
    #expect(throws: (any Error).self) { try agents.install(try scratch(), schedules: [daily("Nightly", at: 3, 0)]) }
    #expect(calls.list.isEmpty)
  }
}

@MainActor
struct ScheduleNotifierTests {
  @Test func tellsOnlyAboutNewRuns() throws {
    let project = try scratch().path
    let defaults = try #require(UserDefaults(suiteName: "jevision-schedule-tests-\(UUID().uuidString)"))
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
