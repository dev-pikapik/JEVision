import Foundation
import JEVisionCore
import JEVisionHands

/// The launchd jobs that start `jevision schedule run <project>` at the times of a project's
/// schedules: one property list per project in the user's LaunchAgents folder, loaded into the
/// user's session.
struct ScheduleAgents {
  /// Startup recovery and edits of an open project share this lane, including across windows.
  private static let mutationLock = NSLock()
  /// Where the property lists go.
  var folder = FileManager.default.homeDirectoryForCurrentUser.appending(path: "Library/LaunchAgents")
  var bundleID = AISettings.appDomain
  /// The `jevision` tool the jobs start.
  var tool = CommandLineTool.bundled
  /// Runs `launchctl` with these arguments; throws what it said when it fails.
  var launchctl: @Sendable ([String]) throws -> Void = Self.runLaunchctl
  /// How long replacing a job waits for launchd to finish taking the old one out.
  var patience: Duration = .seconds(3)
  var checkPreferences: @Sendable () throws -> Void = { try AppPreferences.checkApp() }
  /// The jobs are the person's own, in their session and under the app's name, so a separate
  /// profile neither installs nor reads them.
  var isAvailable = !Profile.current.isolated

  struct Failure: LocalizedError {
    let errorDescription: String?
    /// What launchctl said, for whoever wants the details.
    var detail: String?
  }

  struct ReconciliationFailure: Equatable, Sendable {
    let project: String
    let message: String
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
    guard isAvailable else { return (false, false) }
    return Self.mutationLock.withLock {
      guard (try? checkPreferences()) != nil else { return (false, false) }
      guard managedProject(in: file(for: project)) == Self.path(project) else { return (false, false) }
      let installed = try? String(contentsOf: file(for: project), encoding: .utf8)
      let wanted = try? agent(for: project, schedules: schedules)
      guard let wanted, installed == wanted.plist else { return (true, false) }
      return (true, (try? launchctl(["print", service(for: project)])) != nil)
    }
  }

  /// A valid managed file records that this Mac was explicitly asked to run this project.
  /// Reading it does not call launchd; callers on the main thread can decide whether to queue work.
  func hasManagedJob(for project: URL) -> Bool {
    isAvailable && (try? checkPreferences()) != nil && managedProject(in: file(for: project)) == Self.path(project)
  }

  /// Installs or replaces the job of `project` so it starts at the times of `schedules`; with
  /// none, removes it. A job launchd already has as it should be is left running.
  func install(_ project: URL, schedules: [ProjectSchedule]) throws {
    try Self.mutationLock.withLock { try installUnlocked(project, schedules: schedules) }
  }

  private func installUnlocked(_ project: URL, schedules: [ProjectSchedule]) throws {
    guard isAvailable else {
      throw Failure(errorDescription: String(localized: "Schedules don’t run in a separate profile."))
    }
    try checkPreferences()
    try checkExistingFile(for: project)
    guard let agent = try agent(for: project, schedules: schedules) else { return try removeUnlocked(project) }
    guard tool.unavailableReason == nil else {
      throw Failure(errorDescription: String(localized: "This copy of JEVision can’t run tests on a schedule."))
    }
    let file = folder.appending(path: agent.label + ".plist")
    let service = service(for: project)
    // launchd has this very job: replacing it would only stop a run in progress.
    if (try? String(contentsOf: file, encoding: .utf8)) == agent.plist, (try? launchctl(["print", service])) != nil {
      return
    }
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    // launchd keeps the old job until it is taken out, and can still be taking it out after
    // bootout returns; until then, bootstrap fails with “5: Input/output error”.
    try? launchctl(["bootout", service])
    guard waitUntilGone(service) else {
      throw Failure(
        errorDescription: String(localized: "macOS was still stopping the previous version of these schedules."))
    }
    try checkPreferences()
    do {
      try agent.plist.write(to: file, atomically: true, encoding: .utf8)
      do {
        try checkPreferences()
        try launchctl(["bootstrap", "gui/\(getuid())", file.path(percentEncoded: false)])
      } catch {
        throw Failure(
          errorDescription: String(
            localized:
              "macOS didn’t accept the schedules. In System Settings, open General › Login Items & Extensions and check that JEVision is allowed in the background."
          ),
          detail: error.localizedDescription)
      }
    } catch {
      // The old job is gone or on its way out: without its file, the schedules read as not
      // running, rather than as running the old way.
      try? FileManager.default.removeItem(at: file)
      throw error
    }
  }

  /// Waits up to `patience` until launchd no longer has `service`; whether it is gone.
  private func waitUntilGone(_ service: String) -> Bool {
    let deadline = ContinuousClock.now + patience
    while (try? launchctl(["print", service])) != nil {
      guard ContinuousClock.now < deadline else { return false }
      Thread.sleep(forTimeInterval: 0.05)
    }
    return true
  }

  /// Stops the job of `project` and deletes it.
  func remove(_ project: URL) throws {
    try Self.mutationLock.withLock { try removeUnlocked(project) }
  }

  private func removeUnlocked(_ project: URL) throws {
    guard isAvailable else { return }
    try checkPreferences()
    let file = file(for: project)
    guard (try? FileManager.default.attributesOfItem(atPath: file.path)) != nil else { return }
    try checkExistingFile(for: project)
    let service = service(for: project)
    try? launchctl(["bootout", service])
    guard waitUntilGone(service) else {
      throw Failure(errorDescription: String(localized: "macOS was still stopping the previous version of these schedules."))
    }
    try checkPreferences()
    try FileManager.default.removeItem(at: file)
  }

  /// Project folders that have a job of this app.
  func projects() -> [String] {
    guard isAvailable, (try? checkPreferences()) != nil else { return [] }
    let prefix = bundleID + ".schedule."
    let files = (try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)) ?? []
    return files.filter { $0.lastPathComponent.hasPrefix(prefix) && $0.pathExtension == "plist" }
      .compactMap { managedProject(in: $0) }.sorted()
  }

  /// Reconnects only projects already enabled on this Mac. A newer or unreadable schedule/history,
  /// a missing project or a file that isn't ours is never replaced by a guessed empty schedule.
  func reconcile() -> [ReconciliationFailure] {
    guard isAvailable else { return [] }
    return projects().compactMap { path in
      let project = URL(filePath: path)
      do {
        try Self.mutationLock.withLock {
          try checkPreferences()
          // The file may have disappeared or changed since the directory was scanned.
          guard managedProject(in: file(for: project)) == path else { return }
          var directory: ObjCBool = false
          guard FileManager.default.fileExists(atPath: path, isDirectory: &directory), directory.boolValue else {
            throw CocoaError(.fileNoSuchFile, userInfo: [NSFilePathErrorKey: path])
          }
          let schedules = try projectSchedules(project: path)
          _ = try scheduledRuns(project: path)
          try installUnlocked(project, schedules: schedules)
        }
        return nil
      } catch {
        return ReconciliationFailure(project: path, message: error.localizedDescription)
      }
    }
  }

  private func service(for project: URL) -> String { "gui/\(getuid())/\(label(for: project))" }

  private func checkExistingFile(for project: URL) throws {
    let file = file(for: project)
    guard (try? FileManager.default.attributesOfItem(atPath: file.path)) != nil else { return }
    guard managedProject(in: file) == Self.path(project) else {
      throw CocoaError(.fileReadCorruptFile, userInfo: [NSFilePathErrorKey: file.path])
    }
  }

  /// Match both the command and the path-derived identity before reading or changing a job.
  /// A similarly named job or a link to another file does not constitute consent for this one.
  private func managedProject(in file: URL) -> String? {
    guard let values = try? file.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey]),
      values.isRegularFile == true, values.isSymbolicLink != true,
      let data = try? Data(contentsOf: file),
      let plist = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any],
      let label = plist["Label"] as? String,
      let associated = plist["AssociatedBundleIdentifiers"] as? [String], associated == [bundleID],
      let arguments = plist["ProgramArguments"] as? [String], arguments.count == 4,
      arguments[0].hasPrefix("/"), URL(filePath: arguments[0]).lastPathComponent == CommandLineTool.name,
      arguments[1] == "schedule", arguments[2] == "run", arguments[3].hasPrefix("/"),
      !arguments.contains(where: { $0.contains("\0") }),
      Self.path(URL(filePath: arguments[3])) == arguments[3],
      label == self.label(for: URL(filePath: arguments[3])),
      file.lastPathComponent == label + ".plist"
    else { return nil }
    return arguments[3]
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
