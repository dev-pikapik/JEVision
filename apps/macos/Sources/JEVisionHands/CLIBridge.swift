import Foundation

/// `jevision run --through-app`: the command runs inside JEVision.app, so macOS asks the app for
/// its permissions to see the screen and use the mouse and keyboard rather than the terminal.
/// The two sides talk through a folder: `request.json` says what to run, the run writes to `out`
/// and `err`, each line of a `stop` file interrupts it and `status` holds its exit status once
/// it ended.
public enum CLIBridge {
  public static let request = "request.json"
  public static let output = "out"
  public static let errors = "err"
  public static let stop = "stop"
  public static let status = "status"
  /// The argument that makes JEVision.app serve the folder after it.
  public static let serveArgument = "--serve-cli"

  /// What to run: the `jevision` arguments, the folder they run in and the environment of the
  /// command that asked, which the run gets on top of the app's.
  public struct Request: Codable, Equatable, Sendable {
    public var arguments: [String]
    public var directory: String
    public var environment: [String: String]

    public init(arguments: [String], directory: String, environment: [String: String]) {
      self.arguments = arguments
      self.directory = directory
      self.environment = environment
    }
  }

  /// Runs `helper` as `folder`'s request asks, its output in `out` and `err`, interrupts it when
  /// `stop` appears or grows, then writes its exit status to `status` and returns it.
  public static func serve(_ folder: URL, helper: URL) -> Int32 {
    let status = run(folder, helper: helper)
    try? "\(status)\n".write(to: folder.appending(path: Self.status), atomically: true, encoding: .utf8)
    return status
  }

  private static func run(_ folder: URL, helper: URL) -> Int32 {
    let errorFile = try? FileHandle(forWritingTo: folder.appending(path: Self.errors))
    func fail(_ message: String) -> Int32 {
      errorFile?.seekToEndOfFile()
      errorFile?.write(Data("jevision: \(message)\n".utf8))
      return 3
    }
    guard let output = try? FileHandle(forWritingTo: folder.appending(path: Self.output)), let errors = errorFile,
      let data = try? Data(contentsOf: folder.appending(path: Self.request)),
      let request = try? JSONDecoder().decode(Request.self, from: data)
    else { return fail(handsWords("cli.bridge.noRequest", folder.path(percentEncoded: false))) }
    let process = Process()
    process.executableURL = helper
    process.arguments = request.arguments
    process.currentDirectoryURL = URL(filePath: request.directory, directoryHint: .isDirectory)
    process.environment = ProcessInfo.processInfo.environment.merging(request.environment) { $1 }
    process.standardInput = FileHandle.nullDevice
    process.standardOutput = output
    process.standardError = errors
    do {
      try process.run()
    } catch {
      return fail(handsWords("cli.bridge.cannotStart", helper.path(percentEncoded: false), error.localizedDescription))
    }
    // Each line added to `stop` interrupts the run once more: the first stops it with a report,
    // a second one at once.
    let stop = folder.appending(path: Self.stop).path(percentEncoded: false)
    var asked = -1
    while process.isRunning {
      if let size = (try? FileManager.default.attributesOfItem(atPath: stop))?[.size] as? Int, size > asked {
        process.interrupt()
        asked = size
      }
      usleep(50_000)
    }
    process.waitUntilExit()
    // A run ended by a signal exits the way a shell reports it.
    return process.terminationReason == .uncaughtSignal ? 128 + process.terminationStatus : process.terminationStatus
  }
}
