import Foundation
import JEVisionCore
import JEVisionHands
import Testing

@testable import JEVision
@testable import JEVisionCLI

private let valid = #"""
  {"format": 1, "name": "t", "app": "com.example.App", "start": "a",
    "blocks": {"a": {"type": "pressKeys", "keys": "cmd+s", "next": "b"},
      "b": {"type": "check", "reference": {"image": "saved.png"}}}}
  """#
private let brokenEdge = #"""
  {"format": 1, "name": "t", "app": "com.example.App", "start": "a",
    "blocks": {"a": {"type": "pressKeys", "keys": "cmd+s", "next": "gone"}}}
  """#

struct CommandLineArgumentTests {
  @Test func commandsAndOptions() throws {
    #expect(try Command.parse([]) == .help(nil))
    #expect(try Command.parse(["--help"]) == .help(nil))
    #expect(try Command.parse(["help", "run"]) == .help("run"))
    #expect(try Command.parse(["fmt", "-h"]) == .help("fmt"))
    #expect(try Command.parse(["--version"]) == .version)
    #expect(try Command.parse(["validate", "a", "--json", "b"]) == .validate(paths: ["a", "b"], json: true))
    #expect(try Command.parse(["fmt", "--check", "--", "--odd-name"]) == .fmt(paths: ["--odd-name"], check: true))

    let run = try Command.parse([
      "run", "t.flow.json", "--project", "p", "--retries=2", "--start-at", "b", "--junit", "j.xml", "--html", "h",
      "--json",
    ])
    #expect(
      run
        == .run(RunRequest(paths: ["t.flow.json"], project: "p", retries: 2, startAt: "b", junit: "j.xml", html: "h", json: true)))

    #expect(try Command.parse(["run", "t.flow.json", "--env", "staging"]) == .run(RunRequest(paths: ["t.flow.json"], environment: "staging")))
    #expect(try Command.parse(["run", "t.flow.json", "--video", "always"]) == .run(RunRequest(paths: ["t.flow.json"], video: .always)))
    #expect(try Command.parse(["run", "t.flow.json", "--video=on-failure"]) == .run(RunRequest(paths: ["t.flow.json"], video: .onFailure)))
    #expect(try Command.parse(["run", "t.flow.json"]) == .run(RunRequest(paths: ["t.flow.json"], video: nil)))
    #expect(throws: UsageError.self) { try Command.parse(["run", "t.flow.json", "--video", "sometimes"]) }
    var counting = RunRequest(paths: ["t.flow.json"])
    counting.includeQuarantinedFailures = true
    #expect(try Command.parse(["run", "t.flow.json", "--include-quarantined-failures"]) == .run(counting))
  }

  @Test func videoSettings() throws {
    let suite = "jevision-tests-\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    defer { defaults.removePersistentDomain(forName: suite) }
    let settings = VideoSettings(defaults)
    #expect(settings.mode == .onFailure)
    #expect(settings.keepDays == 30)
    defaults.set("always", forKey: VideoSettings.modeKey)
    defaults.set(0, forKey: VideoSettings.keepDaysKey)
    #expect(settings.mode == .always)
    #expect(settings.keepDays == nil)
    defaults.set("never", forKey: VideoSettings.modeKey)
    #expect(settings.mode == .onFailure)
    #expect(VideoMode.all.allSatisfy { VideoMode(name: $0.name) == $0 })
  }

  @Test func bugReport() throws {
    #expect(try Command.parse(["issue"]) == .issue(IssueRequest()))
    #expect(try Command.parse(["issue", "-h"]) == .help("issue"))
    #expect(
      try Command.parse(["issue", "t.flow.json", "--project", "p", "--send", "--title", "Sign in fails", "--comment-on", "#12"])
        == .issue(IssueRequest(target: "t.flow.json", project: "p", send: true, title: "Sign in fails", commentOn: 12)))
    #expect(try Command.parse(["issue", "--send", "--new"]) == .issue(IssueRequest(send: true, newIssue: true)))
    for wrong in [
      ["issue", "a.flow.json", "b.flow.json"], ["issue", "--send", "--comment-on", "twelve"],
      ["issue", "--send", "--comment-on", "12", "--new"], ["issue", "--new"], ["issue", "--comment-on", "12"],
    ] {
      #expect(throws: UsageError.self) { try Command.parse(wrong) }
    }
    #expect(Help.text("issue").contains("--comment-on"))
    #expect(Help.text(nil).contains("issue"))
  }

  @Test func bugReportNeedsAFailedRun() {
    let root = FileManager.default.temporaryDirectory.appending(path: "jevision-issue-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: root) }
    try? FileManager.default.createDirectory(at: root.appending(path: ".jevision"), withIntermediateDirectories: true)
    let request = IssueRequest(project: root.path(percentEncoded: false))
    #expect(IssueCommand(request: request).execute() == .failure, "No run has failed, so nothing is sent")
  }

  @Test func assistantServer() throws {
    #expect(try Command.parse(["mcp"]) == .mcp(project: nil))
    #expect(try Command.parse(["mcp", "--project", "p"]) == .mcp(project: "p"))
    #expect(try Command.parse(["mcp", "-h"]) == .help("mcp"))
    #expect(throws: UsageError.self) { try Command.parse(["mcp", "p"]) }

    let root = FileManager.default.temporaryDirectory.appending(path: "jevision-mcp-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: root) }
    let inner = root.appending(path: "tests/login")
    try FileManager.default.createDirectory(at: inner, withIntermediateDirectories: true)
    #expect(MCPCommand.defaultProject(from: inner.path) == inner.path)
    try "{}".write(to: root.appending(path: "tests/jevision.environments.json"), atomically: true, encoding: .utf8)
    #expect(MCPCommand.defaultProject(from: inner.path) == root.appending(path: "tests").path)
    try FileManager.default.removeItem(at: root.appending(path: "tests/jevision.environments.json"))
    try FileManager.default.createDirectory(at: root.appending(path: ".jevision"), withIntermediateDirectories: true)
    #expect(MCPCommand.defaultProject(from: inner.path) == root.path)
    #expect(MCPCommand.defaultProject(from: "/") == nil)

    let suite = "jevision-tests-\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    defer { defaults.removePersistentDomain(forName: suite) }
    let settings = AISettings(defaults)
    #expect(!settings.assistantsMayRun && !settings.assistantsMayCapture)
    defaults.set(true, forKey: AISettings.assistantsRunKey)
    #expect(settings.assistantsMayRun && !settings.assistantsMayCapture)
  }

  @Test func aiProviderAndModel() throws {
    let run = try Command.parse(["run", "t.flow.json", "--provider", "ollama", "--model", "qwen2.5vl:7b"])
    #expect(run == .run(RunRequest(paths: ["t.flow.json"], provider: "ollama", model: "qwen2.5vl:7b")))
    let help = Help.text("run")
    for provider in AISettings.providers { #expect(help.contains(provider.id)) }
  }

  @Test func dataRows() throws {
    #expect(try Command.parse(["run", "t", "--row", "3"]) == .run(RunRequest(paths: ["t"], rows: DataRowRange(first: 2, last: 2))))
    #expect(try Command.parse(["run", "t", "--rows", "2-4"]) == .run(RunRequest(paths: ["t"], rows: DataRowRange(first: 1, last: 3))))
    #expect(try Command.parse(["run", "t", "--rows", "all"]) == .run(RunRequest(paths: ["t"])))
    #expect(Help.text("run").contains("--rows <which>"))
  }

  @Test(arguments: [
    (["validate"], "name at least one test file or folder"),
    (["validate", "--check", "a"], "there is no option “--check”"),
    (["validate", "--json=yes", "a"], "--json takes no value"),
    (["lint", "a"], "there is no command “lint”"),
    (["help", "lint"], "there is no command “lint”"),
    (["run", "a", "--retries", "two"], "--retries needs a whole number of times, such as 2, not “two”"),
    (["run", "a", "--retries"], "--retries needs a value"),
    (["run", "a", "--junit", "x", "--junit", "y"], "--junit is given twice"),
    (["run", "a", "--row", "1", "--rows", "2"], "use either --row or --rows, not both"),
    (["run", "a", "--rows", "4-2"], "\"4-2\" ends before it starts; write the smaller row first, like 2-4."),
    (["run", "a", "--row", "0"], "\"0\" isn't a choice of data rows; write all, a row like 2 or a range like 2-4."),
    (["run", "a", "b", "--start-at", "x"], "--start-at works with one test file only"),
    (
      ["run", "a", "--provider", "skynet"],
      "there is no AI provider “skynet”; use one of: anthropic, openai, gemini, jev, ollama, lmstudio, openai-compatible"
    ),
  ])
  func usageErrors(arguments: [String], message: String) {
    #expect(throws: UsageError.self) { try Command.parse(arguments) }
    do {
      _ = try Command.parse(arguments)
    } catch {
      #expect(error.message == message)
    }
  }

  @Test func permissionsOverSSHGoToTheSSHServer() {
    #expect(Readiness.isSSHServer("/usr/libexec/sshd-session"))
    #expect(Readiness.isSSHServer("/usr/libexec/sshd-keygen-wrapper"))
    #expect(!Readiness.isSSHServer("/System/Applications/Utilities/Terminal.app"))
  }
}

/// Runs the built `jevision` like a person or a CI script would.
@Suite(.serialized)
struct CommandLineToolTests {
  private static let examples = URL(filePath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .appending(path: "../../examples").standardizedFileURL.path

  @Test func examplesAreValidAndFormatted() throws {
    let validated = try jevision("validate", Self.examples)
    #expect(validated.status == 0)
    #expect(validated.out.contains("sign-in.flow.json: ok"), "every picture the examples point at is there")
    #expect(validated.out.contains("open-mail.flow.json: ok"))
    #expect(validated.out.contains("write-a-note.flow.json: 0 errors, 1 warning"))
    #expect(
      validated.out.contains(
        "  warning Block \"check\", field \"reference.image\": refs/note.png doesn't exist yet"),
      "a missing reference is only a warning: the first run saves it")
    #expect(try jevision("fmt", "--check", Self.examples).status == 0)
  }

  @Test func validateReportsStatusAndIssues() throws {
    let dir = try Scratch()
    let good = try dir.file("good.flow.json", valid)
    let bad = try dir.file("bad.flow.json", brokenEdge)
    _ = try dir.file("saved.png", "png")

    let ok = try jevision("validate", good)
    #expect(ok.status == 0)
    #expect(ok.out == "\(good): ok\n")

    let failed = try jevision("validate", good, bad)
    #expect(failed.status == 1)
    #expect(failed.out.contains("\(bad): 1 error, 1 warning"))
    #expect(failed.out.contains("  error   Block \"a\", edge \"next\": leads to \"gone\", but there is no such block.\n"))

    let missing = try jevision("validate", dir.url.appending(path: "none.flow.json").path)
    #expect(missing.status == 2)
    #expect(missing.err.contains("cannot read: No such file or directory"))
  }

  @Test func validateWarnsAboutMissingFilesNextToTheTest() throws {
    let dir = try Scratch()
    let test = try dir.file("click.flow.json", #"""
      {"format": 1, "name": "t", "app": "com.example.App", "start": "a",
        "blocks": {"a": {"type": "click", "target": {"image": "button.png"}, "next": "b"},
          "b": {"type": "waitFor", "until": {"appears": {"image": "button.png"}}}}}
      """#)
    let missing = try jevision("validate", test)
    #expect(missing.status == 0, "warnings don't fail validation")
    #expect(missing.out == """
      \(test): 0 errors, 2 warnings
        warning Block "a", field "target.image": button.png doesn't exist; the run will stop at this block until it's added.
        warning Block "b", field "until.appears.image": button.png doesn't exist; the run will stop at this block until it's added.

      """)
    _ = try dir.file("button.png", "png")
    #expect(try jevision("validate", test).out == "\(test): ok\n")
  }

  @Test func validateJSONIsMachineReadable() throws {
    let dir = try Scratch()
    _ = try dir.file("bad.flow.json", brokenEdge)
    _ = try dir.file("notes.txt", "not a test")
    let result = try jevision("validate", "--json", dir.url.path)
    #expect(result.status == 1)
    let reports = try #require(JSONSerialization.jsonObject(with: Data(result.out.utf8)) as? [[String: Any]])
    #expect(reports.count == 1, "folders yield only *.flow.json files")
    #expect(reports[0]["valid"] as? Bool == false)
    let issue = try #require((reports[0]["issues"] as? [[String: Any]])?.first)
    #expect(issue["code"] as? String == "flow.edge.unknownTarget")
    #expect(issue["severity"] as? String == "error")
    #expect((issue["params"] as? [String: String])?["target"] == "gone")
    #expect(
      result.out.contains("\"params\": {\n          \"block\": \"a\",\n          \"edge\": \"next\","),
      "keeps the order and layout of the issue")
  }

  @Test func fmtChecksThenFixes() throws {
    let dir = try Scratch()
    let file = try dir.file("t.flow.json", valid)

    let check = try jevision("fmt", "--check", file)
    #expect(check.status == 1)
    #expect(check.out == "\(file): not formatted\n")
    #expect(try String(contentsOfFile: file, encoding: .utf8) == valid, "--check never writes")

    #expect(try jevision("fmt", file).status == 0)
    let formatted = try String(contentsOfFile: file, encoding: .utf8)
    #expect(formatted.hasPrefix("{\n  \"format\": 1,\n") && formatted.hasSuffix("}\n"))

    let again = try jevision("fmt", "--check", file)
    #expect(again.status == 0)
    #expect(again.out.isEmpty)
    #expect(try FileManager.default.contentsOfDirectory(atPath: dir.url.path).count == 1, "no temporary files left")
  }

  @Test func fmtRefusesUnreadableTests() throws {
    let dir = try Scratch()
    let file = try dir.file("t.flow.json", "{ not json")
    let result = try jevision("fmt", file)
    #expect(result.status == 1)
    #expect(result.out.contains("error   Test: not valid JSON at line 1, column 3."))
    #expect(try String(contentsOfFile: file, encoding: .utf8) == "{ not json")
  }

  @Test func usageErrorsExitWith2() throws {
    let result = try jevision("validate")
    #expect(result.status == 2)
    #expect(result.err.contains("Usage: jevision validate"))
    #expect(try jevision("run", "/no/such/test.flow.json").status == 2)
    #expect(try jevision("--version").out == "jevision \(jevisionVersion())\n")
  }

  private func jevision(_ arguments: String...) throws -> (status: Int32, out: String, err: String) {
    let process = Process()
    process.executableURL = Bundle(for: Marker.self).bundleURL.deletingLastPathComponent().appending(path: "JEVisionCLI")
    process.arguments = arguments
    let out = Pipe()
    let err = Pipe()
    process.standardOutput = out
    process.standardError = err
    try process.run()
    let output = out.fileHandleForReading.readDataToEndOfFile()
    let errors = err.fileHandleForReading.readDataToEndOfFile()
    process.waitUntilExit()
    return (process.terminationStatus, String(decoding: output, as: UTF8.self), String(decoding: errors, as: UTF8.self))
  }
}

private final class Marker {}

/// A fresh folder, removed when the test ends.
private final class Scratch {
  let url: URL

  init() throws {
    let made = FileManager.default.temporaryDirectory.appending(path: "jevision-cli-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: made, withIntermediateDirectories: true)
    // The real path, as the runner sees it: /var is a link to /private/var.
    let real = try #require(realpath(made.path, nil))
    defer { free(real) }
    url = URL(filePath: String(cString: real))
  }

  func file(_ name: String, _ text: String) throws -> String {
    let path = url.appending(path: name).path
    try text.write(toFile: path, atomically: false, encoding: .utf8)
    return path
  }

  deinit {
    try? FileManager.default.removeItem(at: url)
  }
}

/// `jevision run` with hands that wait until the run is cancelled.
@Suite(.serialized)
struct CommandLineRunTests {
  @Test func interruptCancelsTheRunAndReportsIt() throws {
    let dir = try Scratch()
    let test = try dir.file(
      "wait.flow.json",
      #"{"format": 1, "name": "Waits", "app": "com.example.App", "start": "open", "blocks": {"open": {"type": "openApp"}}}"#)
    let printed = Lines()
    let progress = Console(write: { printed.append($0) })
    let opening = DispatchSemaphore(value: 0)
    var request = RunRequest(paths: [test], json: true)
    request.junit = dir.url.appending(path: "junit.xml").path
    let session = TestSession(project: dir.url.path, tests: [test], request: request, progress: progress, input: nil) { run in
      WaitingHands(opening: opening, isCancelled: { run.isCancelled() })
    }
    let interrupts = Interrupts.install { session.stop() }
    defer {
      interrupts.forEach { $0.cancel() }
      signal(SIGINT, SIG_DFL)
      signal(SIGTERM, SIG_DFL)
    }

    let finished = DispatchSemaphore(value: 0)
    let results = Box<[TestResult]>()
    Thread.detachNewThread {
      results.value = session.runAll()
      finished.signal()
    }
    try #require(opening.wait(timeout: .now() + 10) == .success, "the test never reached its first block")
    kill(getpid(), SIGINT)
    try #require(finished.wait(timeout: .now() + 10) == .success, "Ctrl-C didn’t stop the run")

    let result = try #require(results.value?.first)
    #expect(result.verdict == "cancelled")
    #expect(result.file == "wait.flow.json")
    #expect(printed.text.contains("Stopping…"))
    #expect(printed.text.contains("1 stopped"))

    var command = RunCommand(request: request)
    let output = Lines()
    command.output = Console(write: { output.append($0) })
    command.progress = progress
    #expect(command.finish(results.value ?? []) == .problems)
    let json = try #require(JSONSerialization.jsonObject(with: Data(output.text.utf8)) as? [String: Any])
    #expect(json["cancelled"] as? Int == 1)
    #expect(json["passed"] as? Int == 0)
    let tests = try #require(json["tests"] as? [[String: Any]])
    #expect(tests.first?["verdict"] as? String == "cancelled")
    #expect(tests.first?["name"] as? String == "Waits")
    let report = try #require(tests.first?["report"] as? String)
    #expect(FileManager.default.fileExists(atPath: report))
    let junit = try String(contentsOfFile: try #require(request.junit), encoding: .utf8)
    #expect(junit.contains("<skipped message=\"The run was cancelled.\"/>"))
  }

  @Test func quarantinedFailuresCountOnlyWhenAsked() throws {
    let dir = try Scratch()
    let test = try dir.file(
      "broken.flow.json",
      #"{"format": 1, "name": "Broken", "app": "com.example.App", "start": "open", "blocks": {"open": {"type": "openApp"}}}"#)
    _ = try dir.file(
      "jevision.project.json",
      #"{"format": 1, "quarantine": [{"test": "broken.flow.json", "reason": "Bug 42"}]}"#)

    func run(counting: Bool) throws -> (status: Int32, json: [String: Any], junit: String) {
      // A block the test doesn't have fails the run before the app is touched.
      var request = RunRequest(paths: [test], startAt: "nowhere", json: true)
      request.junit = dir.url.appending(path: "junit.xml").path
      request.includeQuarantinedFailures = counting
      let quiet = Console(write: { _ in })
      let session = TestSession(project: dir.url.path, tests: [test], request: request, progress: quiet, input: nil) { _ in
        WaitingHands(opening: DispatchSemaphore(value: 0), isCancelled: { true })
      }
      var command = RunCommand(request: request)
      let output = Lines()
      command.output = Console(write: { output.append($0) })
      command.progress = quiet
      let status = command.finish(session.runAll()).rawValue
      let json = try #require(JSONSerialization.jsonObject(with: Data(output.text.utf8)) as? [String: Any])
      return (status, json, try String(contentsOfFile: try #require(request.junit), encoding: .utf8))
    }

    let honoured = try run(counting: false)
    #expect(honoured.status == 0)
    #expect(honoured.json["quarantined"] as? Int == 1)
    #expect(honoured.json["failed"] as? Int == 0)
    let entry = try #require((honoured.json["tests"] as? [[String: Any]])?.first)
    #expect(entry["verdict"] as? String == "quarantined")
    #expect(entry["quarantineReason"] as? String == "Bug 42")
    #expect(honoured.junit.contains("<skipped message=\"Quarantined (Bug 42)."))
    #expect(!honoured.junit.contains("<failure"))

    let counted = try run(counting: true)
    #expect(counted.status == 1)
    #expect(counted.junit.contains("<failure"))
  }

  @Test func planFindsTestsAndTheProject() throws {
    let dir = try Scratch()
    try FileManager.default.createDirectory(at: dir.url.appending(path: ".jevision"), withIntermediateDirectories: true)
    try FileManager.default.createDirectory(at: dir.url.appending(path: "login"), withIntermediateDirectories: true)
    _ = try dir.file("login/a.flow.json", valid)
    _ = try dir.file("login/b.flow.json", valid)
    _ = try dir.file("top.flow.json", valid)

    let plan = try RunCommand.plan(RunRequest(paths: [dir.url.appending(path: "login").path]))
    #expect(plan.project == dir.url.path, "the folder with .jevision above the tests")
    #expect(plan.tests.map { ($0 as NSString).lastPathComponent } == ["a.flow.json", "b.flow.json"])
    let stem = try RunCommand.plan(RunRequest(paths: [dir.url.appending(path: "login/a").path]))
    #expect(stem.tests.map { ($0 as NSString).lastPathComponent } == ["a.flow.json"], "the name the app shows, without .flow.json")

    #expect(throws: UsageError.self) {
      try RunCommand.plan(RunRequest(paths: [dir.url.path], project: dir.url.appending(path: "login").path))
    }
    #expect(throws: UsageError.self) { try RunCommand.plan(RunRequest(paths: [dir.url.appending(path: ".jevision").path])) }

    let login = dir.url.appending(path: "login").path
    #expect(throws: UsageError(message: "there is no environment “staging”: the project has no jevision.environments.json in \(dir.url.path)")) {
      try RunCommand.plan(RunRequest(paths: [login], environment: "staging"))
    }
    _ = try dir.file("jevision.environments.json", #"{"format":1,"environments":{"dev":{},"prod":{}}}"#)
    #expect(try RunCommand.plan(RunRequest(paths: [login], environment: "dev")).project == dir.url.path)
    #expect(throws: UsageError(message: "there is no environment “staging” in jevision.environments.json; use one of: dev, prod")) {
      try RunCommand.plan(RunRequest(paths: [login], environment: "staging"))
    }
    #expect(try RunCommand.plan(RunRequest(paths: [login])).environment == nil)

    _ = try dir.file("jevision.environments.json", #"{"format":1,"default":"prod","environments":{"dev":{},"prod":{}}}"#)
    #expect(try RunCommand.plan(RunRequest(paths: [login])).environment == "prod", "the file's default without --env")
    #expect(try RunCommand.plan(RunRequest(paths: [login], environment: "dev")).environment == "dev")
    _ = try dir.file("jevision.environments.json", #"{"format":1,"default":"qa","environments":{"dev":{}}}"#)
    #expect(throws: UsageError.self) { try RunCommand.plan(RunRequest(paths: [login])) }
  }
}

struct CommandLineInstallTests {
  @Test func installLinksAndReplaces() throws {
    let dir = try Scratch()
    let tool = CommandLineTool(tool: URL(filePath: "/bin/echo"))
    let bin = dir.url.appending(path: "bin")
    #expect(tool.state(in: [bin]) == .notInstalled)

    try tool.install(in: bin)
    let link = bin.appending(path: "jevision")
    #expect(tool.state(in: [bin]) == .installed(link: link))

    // A link left by a removed copy of the app is replaced, not followed.
    try FileManager.default.removeItem(at: link)
    try FileManager.default.createSymbolicLink(atPath: link.path, withDestinationPath: "/no/such/app")
    #expect(tool.state(in: [bin]) == .notInstalled)
    try tool.install(in: bin)
    #expect(try FileManager.default.destinationOfSymbolicLink(atPath: link.path) == "/bin/echo")

    #expect(CommandLineTool(tool: URL(filePath: "/bin/cat")).state(in: [bin]) == .other(link: link, version: nil))
  }
}

private final class WaitingHands: Hands, @unchecked Sendable {
  private let opening: DispatchSemaphore
  private let isCancelled: @Sendable () -> Bool

  init(opening: DispatchSemaphore, isCancelled: @escaping @Sendable () -> Bool) {
    self.opening = opening
    self.isCancelled = isCancelled
  }

  func openApp(app: String) throws(HandsError) {
    opening.signal()
    while !isCancelled() { usleep(10_000) }
  }

  func openItems(items: [Openable], app: String?, newInstance: Bool) throws(HandsError) {}
  func activate(app: String) throws(HandsError) {}
  func quit(app: String) throws(HandsError) {}
  func capture(app: String) throws(HandsError) -> Screenshot { throw .NoWindow(app: app) }
  func locate(app: String, template: Data, minSimilarity: Double) throws(HandsError) -> Point? { nil }
  func click(app: String, at: Point, button: MouseButton, doubleClick: Bool) throws(HandsError) {}
  func hover(app: String, at: Point) throws(HandsError) {}
  func drag(app: String, path: [Point]) throws(HandsError) {}
  func scroll(app: String, at: Point, direction: ScrollDirection, amount: UInt32, unit: ScrollUnit) throws(HandsError) {}
  func typeText(text: String) throws(HandsError) {}
  func pressKeys(keys: String) throws(HandsError) {}
  func userInputDetected() -> Bool { false }
  func startVideo(app: String, path: String, hide: [Rect]) throws(HandsError) {}
  func stopVideo() throws(HandsError) {}
}

private final class Lines: @unchecked Sendable {
  private let lock = NSLock()
  private var parts: [String] = []

  func append(_ part: String) { lock.withLock { parts.append(part) } }
  var text: String { lock.withLock { parts.joined() } }
}

private final class Box<T>: @unchecked Sendable {
  var value: T?
}
