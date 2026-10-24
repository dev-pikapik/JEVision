import Foundation

/// `jevision run --through-app`: the command runs inside JEVision.app, so macOS asks the app for
/// its permissions to see the screen and use the mouse and keyboard rather than the terminal.
/// The two sides talk through a folder: `request.json` says what to run, the run writes to `out`
/// and `err`, each line of a `stop` file interrupts it and `result.json` holds its versioned
/// exit status. The plain `status` file remains available to older commands.
public enum CLIBridge {
  public static let version = 1
  public static let request = "request.json"
  public static let result = "result.json"
  public static let output = "out"
  public static let errors = "err"
  public static let stop = "stop"
  public static let status = "status"
  /// The argument that makes JEVision.app serve the folder after it.
  public static let serveArgument = "--serve-cli"

  public enum ProtocolError: LocalizedError {
    case unsupportedFormat(Int)
    case invalidStatus

    public var errorDescription: String? {
      switch self {
      case .unsupportedFormat(let found):
        handsWords("project.format.tooNew", String(found), String(CLIBridge.version))
      case .invalidStatus:
        handsWords("flow.field.type", "exitStatus")
      }
    }
  }

  private enum HeaderKey: String, CodingKey { case format }

  /// Requirements are checked before the payload. Only requests had an unversioned predecessor.
  private static func checkFormat(_ decoder: any Decoder, legacy: Bool) throws -> Int {
    let header = try decoder.container(keyedBy: HeaderKey.self)
    let found = legacy && !header.contains(.format) ? version : try header.decode(Int.self, forKey: .format)
    guard found > 0 else {
      throw DecodingError.dataCorruptedError(forKey: .format, in: header, debugDescription: "format must be positive")
    }
    guard found <= version else { throw ProtocolError.unsupportedFormat(found) }
    return found
  }

  /// What to run: the `jevision` arguments, the folder they run in and the environment of the
  /// command that asked, which the run gets on top of the app's.
  public struct Request: Codable, Equatable, Sendable {
    public let format: Int
    public var arguments: [String]
    public var directory: String
    public var environment: [String: String]

    public init(arguments: [String], directory: String, environment: [String: String]) {
      self.format = CLIBridge.version
      self.arguments = arguments
      self.directory = directory
      self.environment = environment
    }

    private enum CodingKeys: String, CodingKey { case format, arguments, directory, environment }

    public init(from decoder: any Decoder) throws {
      format = try CLIBridge.checkFormat(decoder, legacy: true)
      let fields = try decoder.container(keyedBy: CodingKeys.self)
      arguments = try fields.decode([String].self, forKey: .arguments)
      directory = try fields.decode(String.self, forKey: .directory)
      environment = try fields.decode([String: String].self, forKey: .environment)
    }
  }

  public struct Result: Codable, Equatable, Sendable {
    public let format: Int
    public let exitStatus: Int32

    public init(exitStatus: Int32) {
      format = CLIBridge.version
      self.exitStatus = exitStatus
    }

    private enum CodingKeys: String, CodingKey { case format, exitStatus }

    public init(from decoder: any Decoder) throws {
      format = try CLIBridge.checkFormat(decoder, legacy: false)
      let fields = try decoder.container(keyedBy: CodingKeys.self)
      exitStatus = try fields.decode(Int32.self, forKey: .exitStatus)
      guard (0...255).contains(exitStatus) else { throw ProtocolError.invalidStatus }
    }
  }

  /// A modern result takes precedence, including when it is unreadable or needs a newer reader.
  /// Falling back only when it is absent keeps older applications usable without hiding errors.
  public static func readStatus(in folder: URL) throws -> Int32? {
    let result = folder.appending(path: Self.result)
    if FileManager.default.fileExists(atPath: result.path(percentEncoded: false)) {
      return try JSONDecoder().decode(Result.self, from: Data(contentsOf: result)).exitStatus
    }
    let status = folder.appending(path: Self.status)
    guard FileManager.default.fileExists(atPath: status.path(percentEncoded: false)) else { return nil }
    let text = try String(contentsOf: status, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
    guard let code = Int32(text), (0...255).contains(code) else { throw ProtocolError.invalidStatus }
    return code
  }

  /// Runs `helper` as `folder`'s request asks, its output in `out` and `err`, interrupts it when
  /// `stop` appears or grows, then writes its exit status to `status` and returns it.
  public static func serve(_ folder: URL, helper: URL) -> Int32 {
    let status = run(folder, helper: helper)
    // Publish the versioned result first: a new reader must never accept the legacy status
    // before it has had the chance to check the result's format.
    if let data = try? JSONEncoder().encode(Result(exitStatus: status)) {
      try? data.write(to: folder.appending(path: Self.result), options: .atomic)
    }
    try? "\(status)\n".write(to: folder.appending(path: Self.status), atomically: true, encoding: .utf8)
    return status
  }

  private static func run(_ folder: URL, helper: URL) -> Int32 {
    let errorFile = try? FileHandle(forWritingTo: folder.appending(path: Self.errors))
    func fail(_ message: String, status: Int32 = 3) -> Int32 {
      errorFile?.seekToEndOfFile()
      errorFile?.write(Data("jevision: \(message)\n".utf8))
      return status
    }
    guard let output = try? FileHandle(forWritingTo: folder.appending(path: Self.output)), let errors = errorFile,
      let data = try? Data(contentsOf: folder.appending(path: Self.request))
    else { return fail(handsWords("cli.bridge.noRequest", folder.path(percentEncoded: false))) }
    let request: Request
    do {
      request = try JSONDecoder().decode(Request.self, from: data)
    } catch let error as ProtocolError {
      return fail(error.localizedDescription, status: 5)
    } catch {
      return fail(handsWords("cli.bridge.noRequest", folder.path(percentEncoded: false)))
    }
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
