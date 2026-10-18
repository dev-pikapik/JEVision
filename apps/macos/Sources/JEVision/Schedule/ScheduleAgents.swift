import Foundation
import JEVisionCore
import JEVisionHands

/// The launchd jobs that start `jevision schedule run <project>` at the times of a project's
/// schedules: one property list per project in the user's LaunchAgents folder, loaded into the
/// user's session.
struct ScheduleAgents {
  /// Where the property lists go.
  var folder = FileManager.default.homeDirectoryForCurrentUser.appending(path: "Library/LaunchAgents")
  var bundleID = AISettings.appDomain
  /// The `jevision` tool the jobs start.
  var tool = CommandLineTool.bundled
  /// Runs `launchctl` with these arguments; throws what it said when it fails.
  var launchctl: ([String]) throws -> Void = Self.runLaunchctl

  struct Failure: LocalizedError {
    let errorDescription: String?
  }

  func label(for project: URL) -> String {
    scheduleAgentLabel(bundleId: bundleID, project: Self.path(project))
  }

  func file(for project: URL) -> URL {
    folder.appending(path: label(for: project) + ".plist")
  }

  /// The job `schedules` need, or `nil` when there are none to run.
  func agent(for project: URL, schedules: [ProjectSchedule]) throws -> LaunchAgent? {
    guard !schedules.isEmpty else { return nil }
    return try scheduleLaunchAgent(
      bundleId: bundleID, project: Self.path(project), tool: tool.tool.path(percentEncoded: false), schedules: schedules)
  }

  /// Whether the job of `project` is installed, and whether it is the one `schedules` need.
  func state(of project: URL, schedules: [ProjectSchedule]) -> (installed: Bool, current: Bool) {
    let installed = try? String(contentsOf: file(for: project), encoding: .utf8)
    let wanted = try? agent(for: project, schedules: schedules)
    return (installed != nil, installed == wanted?.plist)
  }

  /// Installs or replaces the job of `project` so it starts at the times of `schedules`; with
  /// none, removes it.
  func install(_ project: URL, schedules: [ProjectSchedule]) throws {
    guard tool.unavailableReason == nil else {
      throw Failure(errorDescription: String(localized: "This copy of JEVision can’t run tests on a schedule."))
    }
    guard let agent = try agent(for: project, schedules: schedules) else { return try remove(project) }
    let file = folder.appending(path: agent.label + ".plist")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    // launchd keeps the old job until it is taken out.
    try? launchctl(["bootout", "gui/\(getuid())/\(agent.label)"])
    try agent.plist.write(to: file, atomically: true, encoding: .utf8)
    try launchctl(["bootstrap", "gui/\(getuid())", file.path(percentEncoded: false)])
  }

  /// Stops the job of `project` and deletes it.
  func remove(_ project: URL) throws {
    let file = file(for: project)
    guard FileManager.default.fileExists(atPath: file.path(percentEncoded: false)) else { return }
    try? launchctl(["bootout", "gui/\(getuid())/\(label(for: project))"])
    try FileManager.default.removeItem(at: file)
  }

  /// Project folders that have a job of this app.
  func projects() -> [String] {
    let prefix = bundleID + ".schedule."
    let files = (try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)) ?? []
    return files.filter { $0.lastPathComponent.hasPrefix(prefix) && $0.pathExtension == "plist" }.compactMap { file in
      guard let data = try? Data(contentsOf: file),
        let plist = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any],
        let arguments = plist["ProgramArguments"] as? [String], arguments.count == 4
      else { return nil }
      return arguments[3]
    }
  }

  static func path(_ project: URL) -> String {
    project.standardizedFileURL.path(percentEncoded: false)
  }

  private static func runLaunchctl(_ arguments: [String]) throws {
    let process = Process()
    process.executableURL = URL(filePath: "/bin/launchctl")
    process.arguments = arguments
    let errors = Pipe()
    process.standardError = errors
    process.standardOutput = FileHandle.nullDevice
    try process.run()
    let said = errors.fileHandleForReading.readDataToEndOfFile()
    process.waitUntilExit()
    guard process.terminationStatus != 0 else { return }
    let text = String(decoding: said, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
    throw Failure(errorDescription: text.isEmpty ? "launchctl \(arguments[0]) failed (\(process.terminationStatus))" : text)
  }
}
