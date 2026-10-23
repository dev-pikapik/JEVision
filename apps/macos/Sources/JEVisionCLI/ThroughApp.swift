import AppKit
import Foundation
import JEVisionHands

/// `jevision run --through-app`: JEVision.app runs the command, so macOS asks the app for the
/// permissions to see the screen and use the mouse and keyboard, and this command shows what the
/// run prints and ends with its exit status. How the two talk is in `CLIBridge`.
struct ThroughApp {
  /// The command's arguments without `--through-app` and `--app`, which only this side reads.
  let arguments: [String]
  let app: URL
  var directory = FileManager.default.currentDirectoryPath
  var environment = ProcessInfo.processInfo.environment
  /// Starts `app` serving the folder with `environment`; returns whether it still runs.
  var open: (_ app: URL, _ folder: URL, _ environment: [String: String]) throws -> () -> Bool = Self.open
  /// Makes Ctrl-C and SIGTERM ask the run to stop through the `stop` file.
  var interrupts: (_ stop: URL) -> [DispatchSourceSignal] = Self.forwardInterrupts
  var output = FileHandle.standardOutput
  var errors = FileHandle.standardError

  struct Failure: LocalizedError {
    let message: String
    var errorDescription: String? { message }
  }

  /// `arguments` without `--through-app` and `--app <path>`.
  static func forwarded(_ arguments: [String]) -> [String] {
    var kept: [String] = []
    var rest = arguments[...]
    while let argument = rest.popFirst() {
      if argument == "--" { return kept + [argument] + rest }
      if argument == "--through-app" || argument.hasPrefix("--app=") { continue }
      if argument == "--app" {
        _ = rest.popFirst()
        continue
      }
      kept.append(argument)
    }
    return kept
  }

  /// The JEVision.app to run in: the one this `jevision` is part of, then `given` (`--app`),
  /// `JEVISION_APP`, /Applications and ~/Applications. A relative path is in `current`.
  static func findApp(
    executable: URL?, given: String?, environment: [String: String], home: URL, current: String
  ) throws(UsageError) -> URL {
    if let executable {
      let helpers = URL(filePath: RunCommand.realPath(executable.path(percentEncoded: false)) ?? executable.path(percentEncoded: false))
        .deletingLastPathComponent()
      let app = helpers.deletingLastPathComponent().deletingLastPathComponent()
      if helpers.lastPathComponent == "Helpers", helpers.deletingLastPathComponent().lastPathComponent == "Contents",
        app.pathExtension == "app"
      {
        return app
      }
    }
    for named in [given, environment["JEVISION_APP"]].compactMap({ $0 }).filter({ !$0.isEmpty }) {
      let path = absolutePath(named, in: current)
      guard isFolder(path) else { throw UsageError(message: words("cli.error.noAppAt", path), command: "run") }
      return URL(filePath: path, directoryHint: .isDirectory)
    }
    let installed = [URL(filePath: "/Applications"), home.appending(path: "Applications")].map {
      $0.appending(path: "JEVision.app", directoryHint: .isDirectory)
    }
    guard let found = installed.first(where: { isFolder($0.path(percentEncoded: false)) }) else {
      throw UsageError(message: words("cli.error.noApp"), command: "run")
    }
    return found
  }

  /// Hands the run to the app and waits for it to end; its exit status, or 2 when it couldn't
  /// run or ended without saying how.
  func execute() -> Int32 {
    do {
      return try bridge()
    } catch {
      try? errors.write(contentsOf: Data("jevision: \(error.message)\n".utf8))
      return ExitStatus.failure.rawValue
    }
  }

  private func bridge() throws(Failure) -> Int32 {
    let folder = FileManager.default.temporaryDirectory.appending(path: "jevision-bridge-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: folder) }
    var environment = environment
    // The app starts in /, where a relative profile would be another folder.
    if let profile = Profile(environment: environment, current: directory).folder {
      environment[Profile.variable] = profile.path(percentEncoded: false)
    }
    let request = CLIBridge.Request(arguments: arguments, directory: directory, environment: environment)
    do {
      // The request holds the environment, secrets included: only this user may read it.
      try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
      try JSONEncoder().encode(request).write(to: folder.appending(path: CLIBridge.request))
      for name in [CLIBridge.output, CLIBridge.errors] {
        try Data().write(to: folder.appending(path: name))
      }
    } catch {
      throw Failure(message: words("cli.bridge.noFolder", folder.path(percentEncoded: false), error.localizedDescription))
    }
    let isRunning: () -> Bool
    do {
      isRunning = try open(app, folder, environment)
    } catch {
      throw Failure(message: words("cli.bridge.cannotOpen", app.path(percentEncoded: false), error.localizedDescription))
    }
    let sources = interrupts(folder.appending(path: CLIBridge.stop))
    defer { sources.forEach { $0.cancel() } }
    let shown = [(CLIBridge.output, output), (CLIBridge.errors, errors)].compactMap { name, to in
      (try? FileHandle(forReadingFrom: folder.appending(path: name))).map { (from: $0, to: to) }
    }
    func show() {
      for file in shown {
        if let data = try? file.from.readToEnd(), !data.isEmpty { try? file.to.write(contentsOf: data) }
      }
    }
    let status = folder.appending(path: CLIBridge.status)
    while true {
      // The status comes last, so what the run printed before it is all there to show.
      let ended = try? String(contentsOf: status, encoding: .utf8)
      let gone = ended == nil && !isRunning()
      show()
      if let ended, let code = Int32(ended.trimmingCharacters(in: .whitespacesAndNewlines)) { return code }
      if gone, (try? String(contentsOf: status, encoding: .utf8)) == nil {
        throw Failure(message: words("cli.bridge.noStatus", app.path(percentEncoded: false)))
      }
      usleep(50_000)
    }
  }

  /// Ctrl-C and SIGTERM ask the run in the app to stop, each one by a line in the `stop` file:
  /// the first stops the run with a report, a second one at once.
  private static func forwardInterrupts(_ stop: URL) -> [DispatchSourceSignal] {
    [SIGINT, SIGTERM].map { number in
      signal(number, SIG_IGN)
      let source = DispatchSource.makeSignalSource(signal: number, queue: .global())
      source.setEventHandler { askToStop(stop) }
      source.resume()
      return source
    }
  }

  /// Adds a line to the `stop` file, making it if there is none.
  static func askToStop(_ stop: URL) {
    let path = stop.path(percentEncoded: false)
    if !FileManager.default.fileExists(atPath: path) { FileManager.default.createFile(atPath: path, contents: nil) }
    guard let file = FileHandle(forWritingAtPath: path) else { return }
    file.seekToEndOfFile()
    file.write(Data("\n".utf8))
    try? file.close()
  }

  /// Opens a new copy of `app` in the background, without a window, serving `folder`.
  private static func open(_ app: URL, folder: URL, environment: [String: String]) throws -> () -> Bool {
    let config = NSWorkspace.OpenConfiguration()
    config.createsNewApplicationInstance = true
    config.activates = false
    config.addsToRecentItems = false
    config.arguments = [CLIBridge.serveArgument, folder.path(percentEncoded: false)]
    config.environment = environment
    let opened = Opened()
    let done = DispatchSemaphore(value: 0)
    NSWorkspace.shared.openApplication(at: app, configuration: config) { running, error in
      opened.pid = running?.processIdentifier
      opened.error = error
      done.signal()
    }
    done.wait()
    if let error = opened.error { throw error }
    // A pid of -1 would ask about every process of this user.
    return { opened.pid.map { $0 > 0 && kill($0, 0) == 0 } ?? false }
  }

  private final class Opened: @unchecked Sendable {
    var pid: pid_t?
    var error: (any Error)?
  }
}
