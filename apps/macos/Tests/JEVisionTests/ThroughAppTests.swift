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
