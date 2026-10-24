import Foundation
import JEVisionHands
import Testing

@testable import JEVisionCLI

struct ThroughAppTests {
  private let shell = URL(filePath: "/bin/sh")

  private func scratch() throws -> URL {
    let made = FileManager.default.temporaryDirectory.appending(path: "jevision-bridge-test-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: made, withIntermediateDirectories: true)
    let real = try #require(realpath(made.path, nil))
    defer { free(real) }
    return URL(filePath: String(cString: real), directoryHint: .isDirectory)
  }

  /// A bridge folder as the command line leaves it for the app.
  private func bridge(_ script: String, in directory: URL, environment: [String: String] = [:]) throws -> URL {
    let folder = try scratch()
    let request = CLIBridge.Request(
      arguments: ["-c", script], directory: directory.path(percentEncoded: false), environment: environment)
    try JSONEncoder().encode(request).write(to: folder.appending(path: CLIBridge.request))
    try Data().write(to: folder.appending(path: CLIBridge.output))
    try Data().write(to: folder.appending(path: CLIBridge.errors))
    return folder
  }

  private func text(_ name: String, in folder: URL) -> String {
    (try? String(contentsOf: folder.appending(path: name), encoding: .utf8)) ?? ""
  }

  @Test func theFlagsStayOnThisSideAndTheRestGoesToTheApp() throws {
    #expect(
      ThroughApp.forwarded(["--lang", "ru", "run", "t", "--through-app", "--app", "/A.app", "--app-copy", "a=B.app", "--json"])
        == ["--lang", "ru", "run", "t", "--app-copy", "a=B.app", "--json"])
    #expect(ThroughApp.forwarded(["run", "--app=/A.app", "--through-app", "--", "--through-app"]) == ["run", "--", "--through-app"])

    guard case .run(let request) = try Command.parse(["run", "t", "--through-app", "--app", "J.app"]) else {
      throw UsageError(message: "not a run")
    }
    #expect(request.throughApp)
    #expect(request.app == "J.app")
    #expect(throws: UsageError(message: "--app works only with --through-app", command: "run")) {
      try Command.parse(["run", "t", "--app", "J.app"])
    }
    #expect(Help.text("run").contains("--through-app"))
  }

  @Test func theAppIsTheOneThisCommandIsPartOfThenTheNamedThenTheInstalledOne() throws {
    let folder = try scratch()
    defer { try? FileManager.default.removeItem(at: folder) }
    let bundled = folder.appending(path: "Bundled.app/Contents/Helpers/jevision")
    try FileManager.default.createDirectory(at: bundled.deletingLastPathComponent(), withIntermediateDirectories: true)
    try Data().write(to: bundled)
    let named = folder.appending(path: "Named.app", directoryHint: .isDirectory)
    try FileManager.default.createDirectory(at: named, withIntermediateDirectories: true)
    let home = folder.appending(path: "home", directoryHint: .isDirectory)
    // Paths compare without a trailing slash and, as `standardizingPath` makes them, without /private.
    func plain(_ path: String) -> String { (path as NSString).standardizingPath }
    let current = plain(folder.path(percentEncoded: false))
    func find(_ executable: URL?, _ given: String?, _ environment: [String: String] = [:]) throws -> String {
      plain(
        try ThroughApp.findApp(executable: executable, given: given, environment: environment, home: home, current: current)
          .path(percentEncoded: false))
    }

    #expect(try find(bundled, "Named.app") == "\(current)/Bundled.app")
    #expect(try find(URL(filePath: "/usr/local/bin/jevision"), "Named.app") == "\(current)/Named.app")
    #expect(try find(nil, nil, ["JEVISION_APP": "\(current)/Named.app"]) == "\(current)/Named.app")
    #expect(throws: UsageError(message: "there is no JEVision app at \(current)/Gone.app", command: "run")) {
      try find(nil, "Gone.app")
    }

    let system = FileManager.default.fileExists(atPath: "/Applications/JEVision.app")
    if system {
      #expect(try find(nil, nil) == "/Applications/JEVision.app")
    } else {
      #expect(throws: UsageError.self) { try find(nil, nil) }
      try FileManager.default.createDirectory(at: home.appending(path: "Applications/JEVision.app"), withIntermediateDirectories: true)
      #expect(try find(nil, nil) == "\(current)/home/Applications/JEVision.app")
    }
  }

  @Test func theAppRunsTheCommandWithItsOutputFolderAndEnvironmentAndSavesItsStatus() throws {
    let directory = try scratch()
    let folder = try bridge(
      "echo hi; echo oops >&2; pwd; echo \"$BRIDGE_TEST\"; exit 3", in: directory, environment: ["BRIDGE_TEST": "from the command"])
    defer {
      try? FileManager.default.removeItem(at: folder)
      try? FileManager.default.removeItem(at: directory)
    }

    #expect(CLIBridge.serve(folder, helper: shell) == 3)
    #expect(text(CLIBridge.output, in: folder) == "hi\n\(directory.path(percentEncoded: false).dropLast())\nfrom the command\n")
    #expect(text(CLIBridge.errors, in: folder) == "oops\n")
    #expect(text(CLIBridge.status, in: folder) == "3\n")
    #expect(try CLIBridge.readStatus(in: folder) == 3)
    let result = try JSONDecoder().decode(CLIBridge.Result.self, from: Data(contentsOf: folder.appending(path: CLIBridge.result)))
    #expect(result.format == 1 && result.exitStatus == 3)
  }

  @Test func newRequestsHaveAVersionAndLegacyRequestsRemainReadableWithoutRewriting() throws {
    let directory = try scratch()
    let folder = try bridge("echo legacy", in: directory)
    defer {
      try? FileManager.default.removeItem(at: folder)
      try? FileManager.default.removeItem(at: directory)
    }
    let file = folder.appending(path: CLIBridge.request)
    var json = try #require(try JSONSerialization.jsonObject(with: Data(contentsOf: file)) as? [String: Any])
    #expect(json["format"] as? Int == 1)
    json.removeValue(forKey: "format")
    json["optionalFutureField"] = ["preserved": true]
    let legacy = try JSONSerialization.data(withJSONObject: json)
    try legacy.write(to: file)
    #expect(CLIBridge.serve(folder, helper: shell) == 0)
    #expect(text(CLIBridge.output, in: folder) == "legacy\n")
    #expect(try Data(contentsOf: file) == legacy)
  }

  @Test func newerRequestRequirementsAreCheckedBeforeThePayloadAndBeforeStartingAnything() throws {
    let directory = try scratch()
    let folder = try bridge("touch ran", in: directory, environment: ["SECRET": "private-demo-key"])
    defer {
      try? FileManager.default.removeItem(at: folder)
      try? FileManager.default.removeItem(at: directory)
    }
    let file = folder.appending(path: CLIBridge.request)
    var json = try #require(try JSONSerialization.jsonObject(with: Data(contentsOf: file)) as? [String: Any])
    json["format"] = 2
    // A future payload need not be decodable by this version at all.
    json["directory"] = ["future": true]
    let bytes = try JSONSerialization.data(withJSONObject: json)
    try bytes.write(to: file)
    #expect(CLIBridge.serve(folder, helper: shell) == 5)
    #expect(try CLIBridge.readStatus(in: folder) == 5)
    #expect(!FileManager.default.fileExists(atPath: directory.appending(path: "ran").path))
    #expect(text(CLIBridge.output, in: folder).isEmpty)
    #expect(!text(CLIBridge.errors, in: folder).contains("private-demo-key"))
    #expect(try Data(contentsOf: file) == bytes)
  }

  @Test func malformedRequestHeadersNeverLaunchTheHelper() throws {
    let directory = try scratch()
    let folder = try bridge("touch ran", in: directory)
    defer {
      try? FileManager.default.removeItem(at: folder)
      try? FileManager.default.removeItem(at: directory)
    }
    let file = folder.appending(path: CLIBridge.request)
    let original = try #require(try JSONSerialization.jsonObject(with: Data(contentsOf: file)) as? [String: Any])
    for format: Any in [NSNull(), false, 0, -1, "1", 1.5] {
      var json = original
      json["format"] = format
      let bytes = try JSONSerialization.data(withJSONObject: json)
      try bytes.write(to: file)
      #expect(CLIBridge.serve(folder, helper: shell) == 3)
      #expect(try Data(contentsOf: file) == bytes)
      #expect(!FileManager.default.fileExists(atPath: directory.appending(path: "ran").path))
    }
  }

  @Test func versionedResultsValidateTheirHeaderBeforeTheirPayload() throws {
    let decoder = JSONDecoder()
    let valid = Data(#"{"format":1,"exitStatus":137,"optionalFutureField":{"a":true}}"#.utf8)
    #expect(try decoder.decode(CLIBridge.Result.self, from: valid) == CLIBridge.Result(exitStatus: 137))
    do {
      _ = try decoder.decode(CLIBridge.Result.self, from: Data(#"{"format":2,"exitStatus":null}"#.utf8))
      Issue.record("A future result must refuse this reader")
    } catch CLIBridge.ProtocolError.unsupportedFormat(let found) {
      #expect(found == 2)
    }
    for json in [#"{"exitStatus":0}"#, #"{"format":null,"exitStatus":0}"#,
      #"{"format":0,"exitStatus":0}"#, #"{"format":true,"exitStatus":0}"#,
      #"{"format":1,"exitStatus":-1}"#, #"{"format":1,"exitStatus":256}"#,
      #"{"format":1,"exitStatus":true}"#, #"{"format":1,"exitStatus":1.5}"#]
    {
      #expect(throws: (any Error).self) { try decoder.decode(CLIBridge.Result.self, from: Data(json.utf8)) }
    }
  }

  @Test func modernResultsTakePrecedenceAndAnUnreadableOneCannotFallBackToLegacySuccess() throws {
    let folder = try scratch()
    defer { try? FileManager.default.removeItem(at: folder) }
    #expect(try CLIBridge.readStatus(in: folder) == nil)
    let legacy = folder.appending(path: CLIBridge.status)
    try "0\n".write(to: legacy, atomically: true, encoding: .utf8)
    #expect(try CLIBridge.readStatus(in: folder) == 0)
    let result = folder.appending(path: CLIBridge.result)
    try JSONEncoder().encode(CLIBridge.Result(exitStatus: 9)).write(to: result)
    #expect(try CLIBridge.readStatus(in: folder) == 9)
    for json in [#"{"format":2}"#, #"{"format":1,"exitStatus":null}"#, "{"] {
      try Data(json.utf8).write(to: result)
      #expect(throws: (any Error).self) { try CLIBridge.readStatus(in: folder) }
    }
  }

  @Test func legacyStatusCodesStayCompatibleButMalformedOnesAreErrors() throws {
    let folder = try scratch()
    defer { try? FileManager.default.removeItem(at: folder) }
    let file = folder.appending(path: CLIBridge.status)
    for code: Int32 in [0, 1, 3, 5, 137, 255] {
      try "\(code)\n".write(to: file, atomically: true, encoding: .utf8)
      #expect(try CLIBridge.readStatus(in: folder) == code)
    }
    for invalid in ["", "bad", "-1", "256", "999999999999999", "1\n2"] {
      try invalid.write(to: file, atomically: true, encoding: .utf8)
      #expect(throws: (any Error).self) { try CLIBridge.readStatus(in: folder) }
    }
  }

  @Test func theCommandAcceptsOldApplicationsAndReportsNewerOrMalformedResults() throws {
    for (result, legacy, expected): (String?, String?, Int32) in [
      (nil, "7\n", 7),
      (#"{"format":1,"exitStatus":9}"#, "0\n", 9),
      (#"{"format":2}"#, "0\n", 5),
      (#"{"format":1,"exitStatus":null}"#, "0\n", 3),
      ("{", "0\n", 3),
      (nil, "broken", 3),
    ] {
      var command = ThroughApp(arguments: ["run", "unused"], app: URL(filePath: "/Applications/JEVision.app"))
      command.interrupts = { _ in [] }
      command.output = FileHandle.nullDevice
      command.errors = FileHandle.nullDevice
      command.open = { _, folder, _ in
        if let result { try Data(result.utf8).write(to: folder.appending(path: CLIBridge.result), options: .atomic) }
        if let legacy { try legacy.write(to: folder.appending(path: CLIBridge.status), atomically: true, encoding: .utf8) }
        return { false }
      }
      #expect(command.execute() == expected)
    }
  }

  @Test func eachStopInterruptsTheRunOnceMore() throws {
    let directory = try scratch()
    let folder = try bridge(
      "trap 'echo stopping; trap \"exit 9\" INT' INT; echo ready; while :; do :; done", in: directory)
    defer {
      try? FileManager.default.removeItem(at: folder)
      try? FileManager.default.removeItem(at: directory)
    }
    let ended = DispatchSemaphore(value: 0)
    let status = Shared<Int32>()
    Thread.detachNewThread { [shell] in
      status.value = CLIBridge.serve(folder, helper: shell)
      ended.signal()
    }
    func waitFor(_ line: String) -> Bool {
      for _ in 0..<200 where !text(CLIBridge.output, in: folder).contains(line) { usleep(25_000) }
      return text(CLIBridge.output, in: folder).contains(line)
    }

    try #require(waitFor("ready"))
    ThroughApp.askToStop(folder.appending(path: CLIBridge.stop))
    try #require(waitFor("stopping"), "the first stop interrupts the run")
    #expect(ended.wait(timeout: .now() + 0.3) == .timedOut, "the run goes on after the first interrupt")
    ThroughApp.askToStop(folder.appending(path: CLIBridge.stop))
    try #require(ended.wait(timeout: .now() + 10) == .success, "the second stop ends the run")
    #expect(status.value == 9)
    #expect(text(CLIBridge.status, in: folder) == "9\n")
  }

  @Test func theCommandShowsWhatTheRunPrintsAndEndsWithItsStatus() throws {
    let directory = try scratch()
    defer { try? FileManager.default.removeItem(at: directory) }
    let output = Pipe()
    let errors = Pipe()
    let opened = Shared<URL>()
    var command = ThroughApp(arguments: ["-c", "echo hi; echo oops >&2; exit 3"], app: URL(filePath: "/Applications/JEVision.app"))
    command.directory = directory.path(percentEncoded: false)
    command.environment = [Profile.variable: "profile"]
    command.interrupts = { _ in [] }
    command.output = output.fileHandleForWriting
    command.errors = errors.fileHandleForWriting
    command.open = { [shell] _, folder, environment in
      opened.value = folder
      #expect(environment[Profile.variable] == directory.appending(path: "profile", directoryHint: .isDirectory).path(percentEncoded: false))
      let done = Shared<Bool>()
      Thread.detachNewThread {
        _ = CLIBridge.serve(folder, helper: shell)
        done.value = true
      }
      return { done.value != true }
    }

    #expect(command.execute() == 3)
    try output.fileHandleForWriting.close()
    try errors.fileHandleForWriting.close()
    #expect(String(decoding: output.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self) == "hi\n")
    #expect(String(decoding: errors.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self) == "oops\n")
    let folder = try #require(opened.value)
    #expect(!FileManager.default.fileExists(atPath: folder.path), "the bridge folder is gone once the run ended")
  }

  @Test func anAppThatEndsWithoutAStatusIsAFailure() throws {
    let errors = Pipe()
    var command = ThroughApp(arguments: ["run", "t"], app: URL(filePath: "/Applications/JEVision.app"))
    command.interrupts = { _ in [] }
    command.output = FileHandle.nullDevice
    command.errors = errors.fileHandleForWriting
    command.open = { _, _, _ in { false } }

    #expect(command.execute() == 3)
    try errors.fileHandleForWriting.close()
    let said = String(decoding: errors.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
    #expect(said == "jevision: /Applications/JEVision.app ended without saying how the run went\n")

    command.errors = FileHandle.nullDevice
    command.open = { _, _, _ in throw CocoaError(.fileNoSuchFile) }
    #expect(command.execute() == 4)
  }
}

private final class Shared<T>: @unchecked Sendable {
  private let lock = NSLock()
  private var stored: T?
  var value: T? {
    get { lock.withLock { stored } }
    set { lock.withLock { stored = newValue } }
  }
}
