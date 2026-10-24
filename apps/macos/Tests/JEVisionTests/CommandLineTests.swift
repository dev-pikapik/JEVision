import AppKit
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
/// A test saved by a JEVision newer than any there is.
private let newer = #"{"format": 99, "name": "t", "app": "com.example.App", "start": "a", "blocks": {}}"#
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
    #expect(try Command.parse(["--version"]) == .version(json: false))
    #expect(try Command.parse(["version", "--json"]) == .version(json: true))
    #expect(try Command.parse(["version", "-h"]) == .help("version"))
    #expect(throws: UsageError(message: "version takes no arguments", command: "version")) {
      try Command.parse(["version", "now"])
    }
    #expect(try Command.parse(["validate", "a", "--json", "b"]) == .validate(paths: ["a", "b"], json: true))
    #expect(try Command.parse(["fmt", "--check", "--", "--odd-name"]) == .fmt(paths: ["--odd-name"], check: true))
    #expect(try Command.parse(["check"]) == .check(project: nil, json: false))
    #expect(try Command.parse(["check", "--json", "p"]) == .check(project: "p", json: true))
    #expect(throws: UsageError.self) { try Command.parse(["check", "a", "b"]) }

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
    var quiet = RunRequest(paths: ["t.flow.json"])
    quiet.notify = false
    #expect(try Command.parse(["run", "t.flow.json", "--no-notify"]) == .run(quiet))
    var chosen = RunRequest(paths: ["t.flow.json"])
    chosen.filter = TestFilter(testType: .smoke, priority: .high, status: .ready, author: "Anna Lee", flaky: true)
    #expect(
      try Command.parse(["run", "t.flow.json", "--type", "Smoke", "--priority=high", "--status", "ready", "--author", "Anna Lee", "--flaky"])
        == .run(chosen))
    #expect(throws: UsageError.self) { try Command.parse(["run", "t.flow.json", "--priority", "urgent"]) }
    #expect(throws: UsageError.self) { try Command.parse(["run", "t.flow.json", "--flaky=yes"]) }
  }

  @Test func migrateTakesOneActionAndOneProject() throws {
    #expect(try Command.parse(["migrate"]) == .migrate(MigrateRequest(action: .migrate(dryRun: false))))
    #expect(
      try Command.parse(["migrate", "--dry-run", "--json", "p"])
        == .migrate(MigrateRequest(action: .migrate(dryRun: true), project: "p", json: true)))
    #expect(try Command.parse(["migrate", "--backups"]) == .migrate(MigrateRequest(action: .backups)))
    #expect(try Command.parse(["migrate", "--restore=x", "p"]) == .migrate(MigrateRequest(action: .restore(id: "x"), project: "p")))
    #expect(try Command.parse(["migrate", "-h"]) == .help("migrate"))
    #expect(throws: UsageError.self) { try Command.parse(["migrate", "--backups", "--restore", "x"]) }
    #expect(throws: UsageError.self) { try Command.parse(["migrate", "a", "b"]) }
    #expect(throws: UsageError.self) { try Command.parse(["migrate", "--restore"]) }
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
    #expect(IssueCommand(request: request).execute() == .usage, "No run has failed, so nothing is sent")
  }

  @Test func aBugReportThatCantBeSentEndsAsTheExitStatusesSay() {
    let wrongRequest: [GitError] = [
      .NotRepository, .NoCommits, .NoRemote, .NoUpstream(branch: "main"), .Detached, .RemoteNotFound, .Rejected,
      .Conflict(paths: ["a"]), .LocalChanges(paths: ["a"]), .EmptyMessage, .NothingToCommit, .BadBranchName(name: "a b"),
      .BranchExists(name: "a"), .NotInCommit(path: "a"), .NotGitHub, .NotPushed(branch: "main"), .PullRequestExists,
      .NothingToPropose(base: "main"),
    ]
    let notReady: [GitError] = [
      .NotInstalled, .NoGitHubCli, .GitHubSignedOut, .SignIn, .SignInExpired, .SignInDenied, .SignInUnavailable,
      .NoIdentity, .NoPermission, .Unreachable, .Failed(message: "HTTP 502"),
    ]
    for error in wrongRequest { #expect(IssueCommand.status(error) == .usage, "retrying won't help: \(error)") }
    for error in notReady { #expect(IssueCommand.status(error) == .notReady, "the Mac or GitHub isn't ready: \(error)") }
  }

  @Test func aFailureInJSONKeepsItsParams() throws {
    let json = Failure(code: "run.env.missing", params: ["name": "PASSWORD", "environment": "Staging"]).json.pretty
    let failure = try #require(JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any])
    #expect(failure["code"] as? String == "run.env.missing")
    #expect(failure["params"] as? [String: String] == ["name": "PASSWORD", "environment": "Staging"])
    let environment = try #require(json.range(of: "\"environment\""))
    let name = try #require(json.range(of: "\"name\""))
    #expect(environment.lowerBound < name.lowerBound, "params come in the same order every time")
  }

  @Test func assistantServer() throws {
    #expect(try Command.parse(["mcp"]) == .mcp(project: nil, action: .serve))
    #expect(try Command.parse(["mcp", "--project", "p"]) == .mcp(project: "p", action: .serve))
    #expect(try Command.parse(["mcp", "--pair", "jevision://assistant?x"]) == .mcp(project: nil, action: .pair("jevision://assistant?x")))
    #expect(try Command.parse(["mcp", "--remote", "Studio"]) == .mcp(project: nil, action: .remote("Studio")))
    #expect(try Command.parse(["mcp", "--remote=Studio", "--project", "Shop"]) == .mcp(project: "Shop", action: .remote("Studio")))
    #expect(try Command.parse(["mcp", "--forget", "Studio"]) == .mcp(project: nil, action: .forget("Studio")))
    #expect(throws: UsageError.self) { try Command.parse(["mcp", "--remote", "Studio", "--forget", "Studio"]) }
    #expect(throws: UsageError.self) { try Command.parse(["mcp", "--pair", "x", "--project", "p"]) }
    #expect(throws: UsageError.self) { try Command.parse(["mcp", "--remote", " "]) }
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
    // Each run an assistant starts is asked about until the person turns that off.
    #expect(settings.assistantsAskFirst)
    defaults.set(false, forKey: AISettings.assistantsAskFirstKey)
    #expect(!settings.assistantsAskFirst)
    defaults.set(true, forKey: AISettings.assistantsAskFirstKey)
    #expect(settings.assistantsAskFirst)
  }

  @Test func aiProviderAndModel() throws {
    let run = try Command.parse(["run", "t.flow.json", "--provider", "ollama", "--model", "qwen2.5vl:7b"])
    #expect(run == .run(RunRequest(paths: ["t.flow.json"], provider: "ollama", model: "qwen2.5vl:7b")))
    let help = Help.text("run")
    for provider in AISettings.providers { #expect(help.contains(provider.id)) }
    let cloudflare = try Command.parse(["run", "t.flow.json", "--provider", "jev-cloudflare"])
    #expect(cloudflare == .run(RunRequest(paths: ["t.flow.json"], provider: "jev-cloudflare")))
    #expect(help.contains("CLOUDFLARE_API_TOKEN") && help.contains("CLOUDFLARE_ACCOUNT_ID"))
    var forced = RunRequest(paths: ["t.flow.json"], provider: "openai")
    forced.forceAI = true
    #expect(try Command.parse(["run", "t.flow.json", "--provider", "openai", "--force-ai"]) == .run(forced))
    #expect(try Command.parse(["run", "--force-ai", "t.flow.json", "--provider", "openai"]) == .run(forced))
    #expect(help.contains("--force-ai"))
  }

  @Test func theForcedAIIsTriedBeforeAnyTestStarts() throws {
    let printed = Lines()
    let progress = Console(write: { printed.append($0) })
    let works: (VisionSettings) throws -> VisionVerdict = { _ in VisionVerdict(passed: true, reason: "A red circle.") }
    func problem(
      _ vision: VisionSettings?, localOnly: Bool = false, ask: (VisionSettings) throws -> VisionVerdict = works
    ) -> String? {
      do {
        try RunCommand.checkForcedAI(vision, localOnly: localOnly, progress: progress, ask: ask)
        return nil
      } catch {
        return error.message
      }
    }
    #expect(
      problem(nil)
        == "jevision: --force-ai needs an AI provider: choose one with --provider or in the app (Settings → AI)")
    var openai = VisionSettings(provider: "openai", model: "gpt-4.1", address: "", key: nil, pictures: true)
    #expect(
      problem(openai)
        == "jevision: --force-ai: there is no API key for OpenAI. Set the OPENAI_API_KEY environment variable, or add the key in the app (Settings → AI)"
    )
    // Screenshots that must stay on this Mac don't go to a cloud provider, not even the test picture.
    #expect(
      problem(openai, localOnly: true)
        == "jevision: --force-ai: OpenAI is a cloud service, but this run may only use AI models on this Mac (Settings → AI or the project’s settings)"
    )
    // A server that isn't there is really asked, and says so.
    let gone = VisionSettings(provider: "ollama", model: "llava", address: "http://127.0.0.1:9", key: nil, pictures: true)
    let unreachable = try #require(problem(gone, ask: { try visionTest(settings: $0) }))
    #expect(
      unreachable.hasPrefix(
        "jevision: --force-ai: Ollama can’t answer checks by meaning, so no test was started. Can’t reach Ollama at http://127.0.0.1:9"
      ), "\(unreachable)")
    #expect(printed.text.isEmpty)

    openai.key = "sk-test"
    #expect(problem(openai) == nil)
    #expect(printed.text == "Every check by meaning asks OpenAI, model gpt-4.1 (--force-ai).\n")
    // A model that sees the test picture wrong is only warned about.
    #expect(problem(openai, ask: { _ in VisionVerdict(passed: false, reason: "A dog.") }) == nil)
    #expect(printed.text.contains("OpenAI (model gpt-4.1) described the test picture wrong: “A dog.” Checks by meaning may go wrong too.\n"))
  }

  @Test func aForcedRunAsksItsOwnAIWhateverTheProjectChooses() throws {
    let dir = try Scratch()
    _ = try dir.file("jevision.project.json", #"{"format": 1, "ai": {"screen": {"provider": "lmstudio"}}}"#)
    let test = try dir.file(
      "look.flow.json",
      #"{"format": 1, "name": "Look", "app": "com.example.App", "start": "check", "blocks": {"check": {"type": "check", "reference": {"compare": "meaning", "expect": "A cart"}}}}"#
    )
    let image = try #require(
      NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: 8, pixelsHigh: 8, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
        isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0))
    let screen = Screenshot(png: try #require(image.representation(using: .png, properties: [:])), scale: 1, dark: false, secure: [])
    // The run's own AI: a server that isn't there, so the failure tells who was asked.
    let own = VisionSettings(provider: "openai-compatible", model: "m", address: "http://127.0.0.1:9", key: nil, pictures: true)
    func failure(forced: Bool) throws -> Failure {
      var request = RunRequest(paths: [test], json: true)
      request.forceAI = forced
      let session = TestSession(
        project: dir.url.path, tests: [test], request: request, progress: Console(write: { _ in }), input: nil, vision: own
      ) { run in
        WaitingHands(opening: DispatchSemaphore(value: 0), isCancelled: { run.isCancelled() }, screen: screen)
      }
      guard case .finished(_, _, let report)? = session.runAll().first?.outcome else { throw UsageError(message: "no report") }
      return try #require(report.attempts.last?.failure)
    }
    // Not forced, the project's provider comes first: LM Studio, which isn't set up here.
    #expect(try failure(forced: false).code == "run.vision.noKey")
    // Forced, the run's own is asked.
    let forced = try failure(forced: true)
    #expect(forced.code == "run.vision.unreachable")
    #expect(forced.params.contains { $0.value.contains("127.0.0.1:9") })
  }

  @Test func parallelTakesAnOptionalNumber() throws {
    let cores = RunRequest.defaultParallel
    #expect(cores >= 1 && cores <= maxParallelRuns())
    func parsed(_ arguments: String...) throws -> RunRequest {
      guard case .run(let request) = try Command.parse(["run"] + arguments) else { throw UsageError(message: "not a run") }
      return request
    }
    #expect(try parsed("t").parallel == nil, "without the flag the tests run one after another")
    #expect(try parsed("t", "--parallel").parallel == cores)
    #expect(try parsed("--parallel", "t").parallel == cores, "a path after --parallel stays a path")
    #expect(try parsed("--parallel", "t").paths == ["t"])
    #expect(try parsed("--parallel", "4", "t").parallel == 4)
    #expect(try parsed("--parallel", "4", "t").paths == ["t"])
    #expect(try parsed("t", "--parallel=3").parallel == 3)
    #expect(try parsed("t", "--parallel", "1").parallel == 1)
    #expect(try parsed("--parallel", "2", "--", "7").paths == ["7"], "a folder named 7 after --")
    #expect(try parsed("--parallel", "--env", "ci", "t").environment == "ci")
    var request = RunRequest(paths: ["t"])
    request.parallel = 2
    #expect(try Command.parse(["run", "t", "--parallel", "2"]) == .run(request))
    #expect(Help.text("run").contains("--parallel [<n>]"))
    #expect(try Command.parse(["run", "--parallel", "-h"]) == .help("run"))
  }

  @Test(arguments: [
    (["run", "t", "--parallel", "0"], "--parallel needs a number of tests from 1 to 16, such as 4, not “0”"),
    (["run", "t", "--parallel=17"], "--parallel needs a number of tests from 1 to 16, such as 4, not “17”"),
    (["run", "t", "--parallel=many"], "--parallel needs a number of tests from 1 to 16, such as 4, not “many”"),
    (["run", "t", "--parallel", "2", "--parallel"], "--parallel is given twice"),
  ])
  func parallelUsageErrors(arguments: [String], message: String) {
    #expect(throws: UsageError(message: message, command: "run")) { try Command.parse(arguments) }
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
    (["run", "a", "--rows", "4-2"], "“4-2” ends before it starts; write the smaller row first, like 2-4."),
    (["run", "a", "--row", "0"], "“0” isn’t a choice of data rows; write all, a row like 2 or a range like 2-4."),
    (["run", "a", "b", "--start-at", "x"], "--start-at works with one test file only"),
    (["run", "a", "--force-ai=yes"], "--force-ai takes no value"),
    (
      ["run", "a", "--provider", "skynet"],
      "there is no AI provider “skynet”; use one of: anthropic, openai, gemini, jev, jev-cloudflare, ollama, lmstudio, openai-compatible"
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
        "  warning Block “check”, field “reference.image”: refs/note.png doesn’t exist yet"),
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
    #expect(failed.out.contains("  error   Block “a”, edge “next”: Leads to “gone”, but there’s no such block.\n"))

    let missing = try jevision("validate", dir.url.appending(path: "none.flow.json").path)
    #expect(missing.status == 2)
    #expect(missing.err.contains("cannot read: No such file or directory"))
  }

  @Test func aMissingBlockIsReportedWithItsVersionAndHowToInstallIt() throws {
    let dir = try Scratch()
    let step = { (package: String) in
      #"{"format": 1, "name": "T", "app": "com.example.App", "start": "a", "blocks": {"a": {"type": "runTest", "test": "blocks/com.acme.sign-in.jevblock/flow.json"\#(package), "inputs": {}}}}"#
    }
    let known = try dir.file("known.flow.json", step(#", "package": {"version": "1.2.0", "source": "https://github.com/acme/sign-in"}"#))
    let result = try jevision("validate", known)
    #expect(result.status == 1)
    #expect(
      result.out.contains(
        "needs the block “com.acme.sign-in 1.2.0”, which isn’t installed in this project. Install it with `jevision blocks install https://github.com/acme/sign-in`, or remove the step."))

    let old = try dir.file("old.flow.json", step(""))
    let oldResult = try jevision("validate", old)
    #expect(oldResult.out.contains("needs the block “com.acme.sign-in”, which isn’t installed"))
    #expect(oldResult.out.contains("`jevision blocks install <folder, .zip file or https address>`"))

    let failure = Failure(
      code: "run.block.notInstalled",
      params: ["block": "a", "package": "com.acme.sign-in", "version": "1.2.0", "source": "https://github.com/acme/sign-in"])
    #expect(
      describeFailure(failure: failure)
        == "Block “a”: the community block com.acme.sign-in 1.2.0 isn’t installed; install it with `jevision blocks install https://github.com/acme/sign-in`, then run the test again.")
  }

  @Test func validateFindsTestsWaitingForEachOther() throws {
    let dir = try Scratch()
    let test = { (name: String, after: String) in
      #"{"format": 1, "name": "\#(name)", "app": "com.example.App", "dependsOn": ["\#(after)"], "start": "a", "blocks": {"a": {"type": "pressKeys", "keys": "cmd+s"}}}"#
    }
    let a = try dir.file("a.flow.json", test("A", "b.flow.json"))
    _ = try dir.file("b.flow.json", test("B", "a.flow.json"))
    let result = try jevision("validate", a)
    #expect(result.status == 1)
    #expect(result.out.contains("in a circle: a.flow.json → b.flow.json → a.flow.json"))
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
        warning Block “a”, field “target.image”: button.png doesn’t exist. The run will stop at this block until it’s added.
        warning Block “b”, field “until.appears.image”: button.png doesn’t exist. The run will stop at this block until it’s added.

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
    let json = try #require(JSONSerialization.jsonObject(with: Data(result.out.utf8)) as? [String: Any])
    #expect(json["jsonVersion"] as? Int == 1)
    let reports = try #require(json["tests"] as? [[String: Any]])
    #expect(reports.count == 1, "folders yield only *.flow.json files")
    #expect(reports[0]["valid"] as? Bool == false)
    let issue = try #require((reports[0]["issues"] as? [[String: Any]])?.first)
    #expect(issue["code"] as? String == "flow.edge.unknownTarget")
    #expect(issue["severity"] as? String == "error")
    #expect((issue["params"] as? [String: String])?["target"] == "gone")
    #expect(issue["message"] as? String == "Block “a”, edge “next”: Leads to “gone”, but there’s no such block.")
    #expect(
      result.out.contains("\"params\": {\n            \"block\": \"a\",\n            \"edge\": \"next\","),
      "keeps the order and layout of the issue")
  }

  @Test func fmtChecksThenFixes() throws {
    let dir = try Scratch()
    let file = try dir.file("t.flow.json", valid)

    let check = try jevision("fmt", "--check", file)
    #expect(check.status == 1)
    #expect(check.out == "\(file): not formatted\n1 test checked, 1 not formatted\n")
    #expect(try String(contentsOfFile: file, encoding: .utf8) == valid, "--check never writes")

    #expect(try jevision("fmt", file).status == 0)
    let formatted = try String(contentsOfFile: file, encoding: .utf8)
    #expect(
      formatted.hasPrefix("{\n  \"$schema\": \"https://dev-pikapik.github.io/JEVision/schemas/test.schema.json\",\n  \"format\": 1,\n")
        && formatted.hasSuffix("}\n"))

    let again = try jevision("fmt", "--check", file)
    #expect(again.status == 0)
    #expect(again.out == "1 test checked, 0 not formatted\n")
    #expect(try FileManager.default.contentsOfDirectory(atPath: dir.url.path).count == 1, "no temporary files left")
  }

  @Test func fmtRefusesUnreadableTests() throws {
    let dir = try Scratch()
    let file = try dir.file("t.flow.json", "{ not json")
    let result = try jevision("fmt", file)
    #expect(result.status == 1)
    #expect(result.out.contains("error   Test: The file isn’t valid JSON. Check line 1, column 3."))
    #expect(try String(contentsOfFile: file, encoding: .utf8) == "{ not json")
  }

  @Test func migrateLabelsTheFilesKeepsABackupAndRestoresIt() throws {
    let dir = try Scratch()
    let file = try dir.file("t.flow.json", valid)
    _ = try dir.file("saved.png", "picture")

    let dry = try jevision("migrate", "--dry-run", "--json", dir.url.path)
    #expect(dry.status == 0)
    let plan = try #require(try JSONSerialization.jsonObject(with: Data(dry.out.utf8)) as? [String: Any])
    #expect(plan["jsonVersion"] as? Int == 1)
    let planned = try #require((plan["files"] as? [[String: Any]])?.first)
    #expect(planned["path"] as? String == "t.flow.json")
    #expect(planned["newLabelFiles"] as? [String] == ["saved.png.jevmeta"])
    #expect(plan["backup"] is NSNull)
    #expect(try String(contentsOfFile: file, encoding: .utf8) == valid, "a dry run writes nothing")
    #expect(!FileManager.default.fileExists(atPath: dir.url.appending(path: "saved.png.jevmeta").path))

    let human = try jevision("migrate", "--dry-run", dir.url.path)
    #expect(human.out.contains("Would change 1 test:\n  t.flow.json — paths to labels: 1, new label files: 1\n"))

    let run = try jevision("migrate", "--json", dir.url.path)
    #expect(run.status == 0)
    let done = try #require(try JSONSerialization.jsonObject(with: Data(run.out.utf8)) as? [String: Any])
    let backup = try #require(done["backup"] as? String)
    let migrated = try String(contentsOfFile: file, encoding: .utf8)
    #expect(migrated.contains("\"format\": 2,") && migrated.contains("\"image\": \"res:"))
    #expect(FileManager.default.fileExists(atPath: dir.url.appending(path: "saved.png.jevmeta").path))
    #expect(try jevision("migrate", dir.url.path).out == "Nothing to change: no test names a file by its path.\n")

    let list = try jevision("migrate", "--backups", dir.url.path)
    #expect(list.status == 0)
    #expect(list.out.hasPrefix("\(backup)  ") && list.out.hasSuffix("  1 file\n"))

    let restore = try jevision("migrate", "--restore", backup, dir.url.path)
    #expect(restore.status == 0)
    #expect(restore.out.hasPrefix("Restored 1 file from backup \(backup)."))
    #expect(try String(contentsOfFile: file, encoding: .utf8) == valid)
    #expect(try jevision("migrate", "--backups", dir.url.path).out.split(separator: "\n").count == 2, "the tests before the restore are kept too")

    let unknown = try jevision("migrate", "--restore", "nope", dir.url.path)
    #expect(unknown.status == 2)
    #expect(unknown.err.hasPrefix("jevision: There’s no backup “nope”"))
  }

  @Test func aFolderWithoutTestsIsReported() throws {
    let dir = try Scratch()
    _ = try dir.file("notes.txt", "not a test")
    for command in ["validate", "fmt"] {
      let result = try jevision(command, dir.url.path)
      #expect(result.status == 2)
      #expect(result.out.isEmpty)
      #expect(result.err == "jevision: there are no tests (*.flow.json files) in \(dir.url.path)\n")
    }
  }

  @Test func jsonTellsWhyTheCommandCouldntDoItsWork() throws {
    let dir = try Scratch()
    try FileManager.default.createDirectory(at: dir.url.appending(path: ".jevision"), withIntermediateDirectories: true)
    _ = try dir.file("notes.txt", "not a test")
    let missing = dir.url.appending(path: "gone").path
    func error(_ arguments: String..., language: String = "en") throws -> (status: Int32, json: [String: Any], err: String) {
      let result = try jevision(arguments, language: language)
      let json = try #require(
        try? JSONSerialization.jsonObject(with: Data(result.out.utf8)) as? [String: Any], "\(arguments): \(result.out)")
      #expect(json["jsonVersion"] as? Int == 1)
      return (result.status, json, result.err)
    }
    func code(_ json: [String: Any]) -> String? { (json["error"] as? [String: Any])?["code"] as? String }
    func message(_ json: [String: Any]) -> String? { (json["error"] as? [String: Any])?["message"] as? String }

    let noTests = "there are no tests (*.flow.json files) in \(dir.url.path)"
    let check = try error("check", dir.url.path, "--json")
    #expect(check.status == 2)
    #expect(code(check.json) == "noTests")
    #expect(message(check.json) == noTests, "the message without the tool's name before it")
    #expect(check.err == "jevision: \(noTests)\n", "standard error says it as before")

    let russian = try error("check", dir.url.path, "--json", language: "ru")
    #expect(code(russian.json) == "noTests", "the code is the same in every language")
    #expect(message(russian.json).map { $0 != noTests && $0.contains(dir.url.path) } == true)

    let noFolder = try error("check", missing, "--json")
    #expect(noFolder.status == 2)
    #expect(code(noFolder.json) == "noFolder")

    let validate = try error("validate", dir.url.path, "--json")
    #expect(validate.status == 2)
    #expect((validate.json["tests"] as? [Any])?.isEmpty == true)
    #expect(code(validate.json) == "noTests")

    let unreadable = try error("validate", missing + ".flow.json", "--json")
    #expect(unreadable.status == 2)
    #expect(code(unreadable.json) == "cannotRead")

    let run = try error("run", missing, "--json")
    #expect(run.status == 2)
    #expect(code(run.json) == "noFileOrFolder")
    #expect(message(run.json) == "there is no file or folder “\(missing)”")

    let arguments = try error("check", dir.url.path, "--json", "--fast")
    #expect(arguments.status == 2)
    #expect(code(arguments.json) == "usage", "wrong arguments say so in JSON too")
  }

  @Test func checkFailsOnFilesOnlyThisMacHas() throws {
    let dir = try Scratch()
    for folder in [".jevision", "Data"] {
      try FileManager.default.createDirectory(at: dir.url.appending(path: folder), withIntermediateDirectories: true)
    }
    _ = try dir.file("Data/users.csv", "name\nann\n")
    _ = try dir.file("button.png", "png")
    let test = { (data: String, image: String) in
      #"{"format": 1, "name": "t", "app": "com.example.App", "start": "a", "blocks": {"#
        + #""a": {"type": "useData", "file": "\#(data)", "body": "b"}, "b": {"type": "click", "target": {"image": "\#(image)"}}}}"#
    }
    _ = try dir.file("sign-in.flow.json", test("data/users.csv", "/Users/ann/button.png"))

    let failed = try jevision("check", dir.url.path)
    #expect(failed.status == 1)
    #expect(
      failed.out == """
        sign-in.flow.json: 2 errors, 0 warnings
          error   Block “a”, field “file”: The test names “data/users.csv”, but the file is called “Data/users.csv”. \
        A Mac treats capital and small letters alike, but Linux and many CI servers don’t. Write the name exactly as the file is called.
          error   Block “b”, field “target.image”: “/Users/ann/button.png” points to a place on this computer only, so the \
        test won’t find the file anywhere else. Put the file in the project and choose it again.
        1 test checked: 2 errors, 0 warnings

        """)
    let json = try jevision("check", "--json", dir.url.path)
    #expect(json.status == 1)
    let checked = try #require(try JSONSerialization.jsonObject(with: Data(json.out.utf8)) as? [String: Any])
    #expect(checked["jsonVersion"] as? Int == 1)
    let tests = try #require(checked["tests"] as? [[String: Any]])
    #expect(tests.map { $0["file"] as? String } == ["sign-in.flow.json"])
    #expect(tests.first?["valid"] as? Bool == false)
    let issues = try #require(tests.first?["issues"] as? [[String: Any]])
    #expect(issues.map { $0["code"] as? String } == ["check.path.case", "check.path.absolute"])

    _ = try dir.file("sign-in.flow.json", test("Data/users.csv", "button.png"))
    let fixed = try jevision("check", dir.url.path)
    #expect(fixed.status == 0)
    #expect(fixed.out == "1 test checked: every file it reads is in the project.\n")

    let none = dir.url.appending(path: "none").path
    let missing = try jevision("check", none)
    #expect(missing.status == 2)
    #expect(missing.err == "jevision: there is no folder “\(none)”\n")

    _ = try dir.file("newer.flow.json", newer)
    #expect(try jevision("check", dir.url.path).status == 5)
  }

  @Test func usageErrorsExitWith2() throws {
    let result = try jevision("validate")
    #expect(result.status == 2)
    #expect(result.err.contains("Usage: jevision validate"))
    #expect(try jevision("run", "/no/such/test.flow.json").status == 2)
    #expect(try jevision("--version").out == "jevision \(jevisionVersion())\n")
  }

  @Test func filesThatCantBeReadOrWrittenExitWith3() throws {
    let dir = try Scratch()
    let locked = try dir.file("locked.flow.json", valid)
    chmod(locked, 0)
    let unreadable = try jevision("validate", locked)
    #expect(unreadable.status == 3)
    #expect(unreadable.err.contains("cannot read: Permission denied"))
    chmod(locked, 0o644)

    let folder = dir.url.appending(path: "kept")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let file = folder.appending(path: "t.flow.json").path
    try valid.write(toFile: file, atomically: false, encoding: .utf8)
    chmod(folder.path, 0o555)
    defer { chmod(folder.path, 0o755) }
    #expect(try jevision("fmt", file).status == 3, "the formatted test can't be written")

    _ = try dir.file("jevision.environments.json", "{ not json")
    #expect(try jevision("run", locked).status == 3)
  }

  @Test func filesFromANewerJEVisionExitWith5() throws {
    let dir = try Scratch()
    let test = try dir.file("newer.flow.json", newer)
    for command in ["validate", "fmt"] {
      let result = try jevision(command, test)
      #expect(result.status == 5, "\(command)")
      #expect(result.out.contains("The file uses format 99 from a newer JEVision"))
    }
    _ = try dir.file("old.flow.json", valid)
    _ = try dir.file("jevision.environments.json", #"{"format": 99, "environments": {}}"#)
    let run = try jevision("run", dir.url.appending(path: "old.flow.json").path)
    #expect(run.status == 5)
    #expect(run.err.contains("jevision.environments.json"))
  }

  @Test func versionTellsWhatThisJEVisionReadsAndSpeaks() throws {
    #expect(try jevision("version").out == "jevision \(jevisionVersion())\n")
    let result = try jevision("version", "--json")
    #expect(result.status == 0)
    let json = try #require(JSONSerialization.jsonObject(with: Data(result.out.utf8)) as? [String: Any])
    #expect(json["jsonVersion"] as? Int == 1)
    #expect(json["version"] as? String == jevisionVersion())
    let formats = try #require(json["formats"] as? [[String: Any]])
    #expect(
      formats.map { $0["file"] as? String } == [
        "*.flow.json", "*.block.json", "*.template.json", "*.jevmeta", "jevision.project.json", "jevision.environments.json", "jevision.schedule.json",
        "block.json", "jevision.blocks.json", "report.json", "jevision.notifications.json", "last-results.json", "schedule-history.json",
      ])
    #expect(formats.first?["version"] as? Int == Int(formatVersion()))
    #expect(json["iphoneProtocol"] as? [String: Int] == ["oldest": 1, "newest": 1])
    #expect(json["mcpProtocols"] as? [String] == ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"])
    #expect(try jevision("help", "version").out.contains("Usage: jevision version [--json]"))
  }

  @Test func langWordsTheMessagesInThatLanguage() throws {
    let result = try jevision("lint", language: "ru")
    #expect(result.status == 2)
    #expect(result.err.hasPrefix("jevision: команды «lint» нет\nИспользование: jevision <команда> [параметры]\n"))
  }

  @Test func langOfALanguageTheCatalogLacksIsAUsageError() throws {
    let result = try jevision("--lang", "xx", "validate", language: nil)
    #expect(result.status == 2)
    #expect(result.out.isEmpty)
    #expect(result.err.contains("xx") && result.err.contains(": en, ru\n"), "names the languages there are")
  }

  /// Runs the built `jevision`, in English unless `language` names another or is `nil`.
  private func jevision(_ arguments: String..., language: String? = "en") throws -> (status: Int32, out: String, err: String) {
    try jevision(arguments, language: language)
  }

  private func jevision(_ arguments: [String], language: String?) throws -> (status: Int32, out: String, err: String) {
    let process = Process()
    process.executableURL = Bundle(for: Marker.self).bundleURL.deletingLastPathComponent().appending(path: "JEVisionCLI")
    process.arguments = (language.map { ["--lang", $0] } ?? []) + arguments
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
  @Test func aReportThatCantBeSavedIsInTheJSONResult() throws {
    var request = RunRequest(paths: ["t"], json: true)
    request.junit = "/dev/null/junit.xml"
    var command = RunCommand(request: request)
    let output = Lines()
    command.output = Console(write: { output.append($0) })
    command.progress = Console(write: { _ in })
    #expect(command.finish([]) == .io)
    let json = try #require(JSONSerialization.jsonObject(with: Data(output.text.utf8)) as? [String: Any])
    let error = try #require(json["error"] as? [String: Any], "\(output.text)")
    #expect(error["code"] as? String == "cannotWrite")
    #expect((error["message"] as? String)?.hasPrefix("/dev/null/junit.xml") == true)
    #expect((json["tests"] as? [Any])?.isEmpty == true, "the result is there as well")
  }

  init() { OwnScreenLock.use() }

  /// The run starts with every thread of the system's shared queue stuck, as a busy machine, or a
  /// test running beside this one, leaves them: it must not wait for one to be free. They are let
  /// go as soon as it has started: stuck for longer, they would stop the tests beside this one.
  @Test(arguments: [false, true])
  func interruptCancelsTheRunAndReportsIt(whileTheSharedQueueIsBusy busy: Bool) throws {
    let stuck = busy ? 300 : 0
    let release = DispatchSemaphore(value: 0)
    var stuckNow = stuck
    func letGo() {
      for _ in 0..<stuckNow { release.signal() }
      stuckNow = 0
    }
    for _ in 0..<stuck { DispatchQueue.global().async { release.wait() } }
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
    try #require(opening.wait(timeout: .now() + hangGuard) == .success, "the test never reached its first block")
    letGo()
    kill(getpid(), SIGINT)
    try #require(finished.wait(timeout: .now() + hangGuard) == .success, "Ctrl-C didn’t stop the run")

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

  @Test func aQuarantineHoldsTheFailureOnlyUntilItsLastDay() throws {
    let dir = try Scratch()
    let test = try dir.file(
      "broken.flow.json",
      #"{"format": 1, "name": "Broken", "app": "com.example.App", "start": "open", "blocks": {"open": {"type": "openApp"}}}"#)

    func status(until: String) throws -> (status: Int32, quarantined: Int?) {
      _ = try dir.file(
        "jevision.project.json",
        #"{"format": 1, "quarantine": [{"test": "broken.flow.json", "reason": "Bug 42", "until": "\#(until)"}]}"#)
      let request = RunRequest(paths: [test], startAt: "nowhere", json: true)
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
      return (status, json["quarantined"] as? Int)
    }

    let lasting = try status(until: "2999-12-31")
    #expect(lasting.status == 0 && lasting.quarantined == 1)
    let today = try status(until: localToday())
    #expect(today.status == 0 && today.quarantined == 1, "the last day still counts")
    let over = try status(until: "2000-01-01")
    #expect(over.status == 1 && over.quarantined == 0, "after its last day the failure fails the run again")
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
    #expect(honoured.json["jsonVersion"] as? Int == 1)
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

  @Test func aTestRunsAfterItsDependencyAndIsBlockedWhenItFails() throws {
    let dir = try Scratch()
    _ = try dir.file(
      "login.flow.json",
      #"{"format": 1, "name": "Login", "app": "com.example.App", "start": "open", "blocks": {"open": {"type": "openApp"}}}"#)
    let inbox = try dir.file(
      "inbox.flow.json",
      #"{"format": 1, "name": "Inbox", "app": "com.example.App", "dependsOn": ["login.flow.json"], "start": "open", "blocks": {"open": {"type": "openApp"}}}"#)
    // A block the test doesn't have fails Login before the app is touched.
    var request = RunRequest(paths: [inbox], startAt: "nowhere", json: true)
    request.junit = dir.url.appending(path: "junit.xml").path
    let quiet = Console(write: { _ in })
    let session = TestSession(project: dir.url.path, tests: [inbox], request: request, progress: quiet, input: nil) { _ in
      WaitingHands(opening: DispatchSemaphore(value: 0), isCancelled: { true })
    }
    var command = RunCommand(request: request)
    let output = Lines()
    command.output = Console(write: { output.append($0) })
    command.progress = quiet
    #expect(command.finish(session.runAll()) == .problems)
    let json = try #require(JSONSerialization.jsonObject(with: Data(output.text.utf8)) as? [String: Any])
    #expect(json["failed"] as? Int == 1)
    #expect(json["blocked"] as? Int == 1)
    let tests = try #require(json["tests"] as? [[String: Any]])
    #expect(tests.map { $0["file"] as? String } == ["login.flow.json", "inbox.flow.json"])
    #expect(tests[0]["addedAsDependency"] as? Bool == true)
    #expect(tests[1]["verdict"] as? String == "blocked")
    let blocker = try #require(tests[1]["blockedBy"] as? [String: Any])
    #expect(blocker["reason"] as? String == "failed")
    #expect(blocker["dependency"] as? String == "login.flow.json")
    #expect(blocker["cause"] as? String == "login.flow.json")
    #expect(blocker["cycle"] is NSNull)
    #expect(blocker["message"] as? String == "Blocked: Login failed")
    let junit = try String(contentsOfFile: try #require(request.junit), encoding: .utf8)
    #expect(junit.contains("<skipped message=\"Blocked: Login failed."))
  }

  @Test func aTestFromANewerJEVisionAsksForIt() throws {
    let dir = try Scratch()
    let test = try dir.file("newer.flow.json", newer)
    let (session, request) = session(dir, [test], parallel: nil)
    var command = RunCommand(request: request)
    let output = Lines()
    command.output = Console(write: { output.append($0) })
    command.progress = Console(write: { _ in })
    #expect(command.finish(session.runAll()).rawValue == 5)
    let json = try #require(JSONSerialization.jsonObject(with: Data(output.text.utf8)) as? [String: Any])
    let failure = try #require((json["tests"] as? [[String: Any]])?.first?["failure"] as? [String: Any])
    #expect(failure["code"] as? String == "run.test.needsNewer")
    #expect((failure["message"] as? String)?.contains("format 99") == true)
  }

  /// What a test that only runs a command and passes ends as: it checked nothing on the screen.
  private static let ended = "nothingChecked"

  /// A test of one command, which leaves the screen alone, in a folder `dir`.
  private func quiet(_ dir: Scratch, _ name: String, _ command: String, after: [String] = []) throws -> String {
    let needs = after.map { "\"\($0)\"" }.joined(separator: ", ")
    let escaped = command.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"")
    return try dir.file(
      "\(name).flow.json",
      """
      {"format": 1, "name": "\(name)", "app": "com.example.App", "dependsOn": [\(needs)], "start": "run",
        "blocks": {"run": {"type": "command", "command": "\(escaped)", "timeout": "60s"}}}
      """)
  }

  private func session(
    _ dir: Scratch, _ tests: [String], parallel: UInt32?, folder: String = ".", console: Console = Console(write: { _ in }),
    hands: @escaping (TestRun) -> any Hands = { run in WaitingHands(opening: DispatchSemaphore(value: 0), isCancelled: { run.isCancelled() }) }
  ) -> (TestSession, RunRequest) {
    var request = RunRequest(paths: tests, json: true)
    request.parallel = parallel
    request.junit = dir.url.appending(path: "junit.xml").path
    return (
      TestSession(project: dir.url.path, tests: tests, folder: folder, request: request, progress: console, input: nil, makeHands: hands),
      request
    )
  }

  @Test func testsThatLeaveTheScreenAloneRunSideBySide() throws {
    let dir = try Scratch()
    // Each waits for the file of the other: one after another, the first would wait for nothing.
    let wait = { (mine: String, theirs: String) in
      "touch \(mine); for i in $(seq 200); do [ -f \(theirs) ] && exit 0; sleep 0.05; done; exit 1"
    }
    let first = try quiet(dir, "first", wait("one", "two"))
    let second = try quiet(dir, "second", wait("two", "one"))
    let (session, _) = session(dir, [first, second], parallel: 2)
    let results = session.runAll()
    #expect(results.map(\.verdict) == [Self.ended, Self.ended])
  }

  @Test func oneAfterAnotherIsTheDefault() throws {
    let dir = try Scratch()
    // The second only passes if the first has ended: it looks for the file the first leaves at its end.
    let first = try quiet(dir, "first", "sleep 0.3; touch done")
    let second = try quiet(dir, "second", "test -f done")
    let (session, _) = session(dir, [first, second], parallel: nil)
    #expect(session.runAll().map(\.verdict) == [Self.ended, Self.ended])
  }

  @Test func withoutParallelTheProjectSettingsChoose() throws {
    let dir = try Scratch()
    _ = try dir.file("jevision.project.json", #"{"format": 1, "runModes": [{"folder": ".", "run": "parallel", "limit": 2}]}"#)
    // Each waits for the file of the other: one after another, the first would wait for nothing.
    let wait = { (mine: String, theirs: String) in
      "touch \(mine); for i in $(seq 200); do [ -f \(theirs) ] && exit 0; sleep 0.05; done; exit 1"
    }
    let first = try quiet(dir, "first", wait("one", "two"))
    let second = try quiet(dir, "second", wait("two", "one"))
    let printed = Lines()
    let (session, _) = session(dir, [first, second], parallel: nil, console: Console(write: { printed.append($0) }))
    #expect(session.runAll().map(\.verdict) == [Self.ended, Self.ended], "side by side, as jevision.project.json sets")
    #expect(printed.text.contains("up to 2 at a time"), "\(printed.text)")
    #expect(
      printed.text.contains(
        "Side by side, as jevision.project.json sets for the whole project. Add --parallel 1 to run them one at a time."))
  }

  @Test func theFolderAboveTheTestsChoosesAndTheRunSaysWhich() throws {
    let dir = try Scratch()
    _ = try dir.file("jevision.project.json", #"{"format": 1, "runModes": [{"folder": "login", "run": "parallel", "limit": 2}]}"#)
    try FileManager.default.createDirectory(at: dir.url.appending(path: "login/deep"), withIntermediateDirectories: true)
    let wait = { (mine: String, theirs: String) in
      "touch \(mine); for i in $(seq 200); do [ -f \(theirs) ] && exit 0; sleep 0.05; done; exit 1"
    }
    let first = try quiet(dir, "login/deep/first", wait("one", "two"))
    let second = try quiet(dir, "login/deep/second", wait("two", "one"))
    let printed = Lines()
    let (session, _) = session(
      dir, [first, second], parallel: nil, folder: "login/deep", console: Console(write: { printed.append($0) }))
    #expect(session.runAll().map(\.verdict) == [Self.ended, Self.ended], "side by side, as set for login")
    #expect(printed.text.contains("as jevision.project.json sets for the folder “login”."), "\(printed.text)")
  }

  @Test func unreadableSettingsRunTheTestsOneAtATime() throws {
    let dir = try Scratch()
    _ = try dir.file("jevision.project.json", "{ not settings")
    let first = try quiet(dir, "first", "true")
    let second = try quiet(dir, "second", "true")
    let printed = Lines()
    let (session, _) = session(dir, [first, second], parallel: nil, console: Console(write: { printed.append($0) }))
    // Every test needs the settings, so each fails and says why.
    let results = session.runAll()
    #expect(results.map(\.verdict) == ["failed", "failed"])
    let codes = results.map { result -> String? in
      guard case .finished(_, _, let report) = result.outcome else { return nil }
      return decidingAttempt(report: report)?.failure?.code
    }
    #expect(codes == ["run.project.unreadable", "run.project.unreadable"])
    #expect(printed.text.contains("jevision.project.json can’t be read ("), "\(printed.text)")
    #expect(printed.text.contains("so the tests run one at a time. Fix the file.\n"))
    #expect(!printed.text.contains("up to"), "not side by side")
  }

  @Test func planNamesTheFolderThatHoldsWhatIsNamed() throws {
    let dir = try Scratch()
    for folder in [".jevision", "login/deep", "other"] {
      try FileManager.default.createDirectory(at: dir.url.appending(path: folder), withIntermediateDirectories: true)
    }
    for test in ["login/a", "login/deep/b", "other/c"] { _ = try dir.file("\(test).flow.json", valid) }
    let folder = { (paths: [String]) in
      try RunCommand.plan(RunRequest(paths: paths.map { dir.url.appending(path: $0).path })).folder
    }
    #expect(try folder(["login"]) == "login")
    #expect(try folder(["login/deep/b.flow.json"]) == "login/deep")
    #expect(try folder(["login/a.flow.json", "login/deep"]) == "login")
    #expect(try folder(["login", "other/c.flow.json"]) == ".")
    #expect(try folder(["."]) == ".")
  }

  @Test func aTestStartsAfterItsDependencyEvenWithPlacesFree() throws {
    let dir = try Scratch()
    _ = try quiet(dir, "setup", "sleep 0.4; touch ready")
    let user = try quiet(dir, "user", "test -f ready", after: ["setup.flow.json"])
    let other = try quiet(dir, "other", "true")
    let (session, _) = session(dir, [user, other], parallel: 4)
    let results = session.runAll()
    #expect(results.map(\.file) == ["setup.flow.json", "user.flow.json", "other.flow.json"])
    #expect(results.map(\.verdict) == Array(repeating: Self.ended, count: 3), "user saw the file setup leaves")
  }

  @Test func aFailedDependencyBlocksItsTestsAndTheRestGoesOn() throws {
    let dir = try Scratch()
    _ = try quiet(dir, "setup", "exit 1")
    let user = try quiet(dir, "user", "true", after: ["setup.flow.json"])
    let other = try quiet(dir, "other", "true")
    let (session, request) = session(dir, [user, other], parallel: 3)
    let results = session.runAll()
    #expect(results.map(\.verdict) == ["failed", "blocked", Self.ended])
    var command = RunCommand(request: request)
    let output = Lines()
    command.output = Console(write: { output.append($0) })
    command.progress = Console(write: { _ in })
    #expect(command.finish(results) == .problems)
    let json = try #require(JSONSerialization.jsonObject(with: Data(output.text.utf8)) as? [String: Any])
    #expect(json["blocked"] as? Int == 1)
    let tests = try #require(json["tests"] as? [[String: Any]])
    let blocker = try #require(tests[1]["blockedBy"] as? [String: Any])
    #expect(blocker["reason"] as? String == "failed")
    #expect(blocker["message"] as? String == "Blocked: setup failed")
  }

  @Test func reportsKeepTheOrderOfTheTestsAndEachTestPrintsInOnePiece() throws {
    let dir = try Scratch()
    let slow = try quiet(dir, "slow", "sleep 0.6")
    let fast = try quiet(dir, "fast", "true")
    let printed = Lines()
    let (session, request) = session(dir, [slow, fast], parallel: 2, console: Console(write: { printed.append($0) }))
    let results = session.runAll()
    #expect(results.map(\.file) == ["slow.flow.json", "fast.flow.json"], "the order the tests were given in")

    let pieces = printed.parts.filter { $0.contains("nothing was checked") }
    #expect(pieces.count == 2, "a test's lines are written together, when it ends")
    guard pieces.count == 2 else { return }
    #expect(pieces[0].contains("fast.flow.json") && pieces[1].contains("slow.flow.json"), "printed as they end")
    #expect(printed.text.contains("up to 2 at a time"))

    var command = RunCommand(request: request)
    let output = Lines()
    command.output = Console(write: { output.append($0) })
    command.progress = Console(write: { _ in })
    #expect(command.finish(results) == .ok)
    let json = try #require(JSONSerialization.jsonObject(with: Data(output.text.utf8)) as? [String: Any])
    let tests = try #require(json["tests"] as? [[String: Any]])
    #expect(tests.map { $0["file"] as? String } == ["slow.flow.json", "fast.flow.json"])
    let junit = try String(contentsOfFile: try #require(request.junit), encoding: .utf8)
    let at = { (name: String) in junit.range(of: "name=\"\(name)\"")?.lowerBound }
    #expect(try #require(at("slow")) < #require(at("fast")), "the JUnit report lists the tests in their order")
  }

  @Test func testsOnTheScreenTakeTurnsBesideQuietOnes() throws {
    let dir = try Scratch()
    let screen = { (name: String) in
      try dir.file(
        "\(name).flow.json",
        #"{"format": 1, "name": "\#(name)", "app": "com.example.App", "start": "open", "blocks": {"open": {"type": "openApp"}}}"#)
    }
    let tests = [try screen("a"), try screen("b"), try screen("c"), try quiet(dir, "q", "sleep 0.2")]
    let counter = ScreenCounter()
    let (session, _) = session(dir, tests, parallel: 4) { run in CountingHands(counter: counter) }
    let results = session.runAll()
    #expect(results.count == 4)
    #expect(counter.entered == 3, "every test on the screen ran")
    #expect(counter.most == 1, "never two tests on the screen at once")
  }

  @Test func stoppingEndsEveryRunningTest() throws {
    let dir = try Scratch()
    let tests = [
      try quiet(dir, "one", "touch one; sleep 30"), try quiet(dir, "two", "touch two; sleep 30"),
      try quiet(dir, "three", "true", after: ["one.flow.json"]),
    ]
    let printed = Lines()
    let (session, _) = session(dir, tests, parallel: 4, console: Console(write: { printed.append($0) }))
    let finished = DispatchSemaphore(value: 0)
    let results = Box<[TestResult]>()
    Thread.detachNewThread {
      results.value = session.runAll()
      finished.signal()
    }
    let both = ["one", "two"].map { dir.url.appending(path: $0).path }
    try eventually("both tests started") { both.allSatisfy { FileManager.default.fileExists(atPath: $0) } }
    session.stop()
    try #require(finished.wait(timeout: .now() + hangGuard) == .success, "stopping didn’t end the tests")
    #expect(results.value?.map(\.verdict) == ["cancelled", "cancelled", "notRun"])
    #expect(printed.text.contains("Stopping…"))
    #expect(printed.text.contains("2 stopped"))
  }

  @Test func planKeepsTheTestsTheFilterChooses() throws {
    let dir = try Scratch()
    try FileManager.default.createDirectory(at: dir.url.appending(path: ".jevision"), withIntermediateDirectories: true)
    func card(_ fields: String) -> String {
      #"{"format": 1, "name": "t", "app": "com.example.App", \#(fields) "start": "open", "blocks": {"open": {"type": "openApp"}}}"#
    }
    _ = try dir.file("critical.flow.json", card(#""priority": "critical", "testType": "smoke", "author": "Anna Lee","#))
    _ = try dir.file("marked.flow.json", card(#""priority": "high", "flaky": true,"#))
    _ = try dir.file("plain.flow.json", card(""))
    func names(_ filter: TestFilter) throws -> [String] {
      var request = RunRequest(paths: [dir.url.path])
      request.filter = filter
      return try RunCommand.plan(request).tests.map { ($0 as NSString).lastPathComponent }
    }

    #expect(try names(TestFilter()).count == 3)
    #expect(try names(TestFilter(priority: .critical)) == ["critical.flow.json"])
    #expect(try names(TestFilter(author: "anna lee")) == ["critical.flow.json"])
    #expect(try names(TestFilter(flaky: true)) == ["marked.flow.json"])
    var request = RunRequest(paths: [dir.url.path])
    request.filter = TestFilter(testType: .smoke)
    #expect(try RunCommand.plan(request).filteredOut == 2)
    #expect(throws: UsageError.self) { try names(TestFilter(priority: .low)) }
  }

  @Test func projectRequirementsStopPlanningBeforeForcedProbes() throws {
    let dir = try Scratch()
    try FileManager.default.createDirectory(at: dir.url.appending(path: ".jevision"), withIntermediateDirectories: true)
    let test = try dir.file("test.flow.json", valid)
    var request = RunRequest(paths: [test])
    request.forceAI = true
    for (json, status) in [
      (#"{"format":99,"ai":false}"#, ExitStatus.needsNewer),
      (#"{"format":1,"minVersion":"9999.0.0","ai":false}"#, .needsNewer),
      (#"{"format":1,"retries":"two"}"#, .io),
    ] {
      _ = try dir.file("jevision.project.json", json)
      do {
        _ = try RunCommand.plan(request)
        Testing.Issue.record("Unsupported settings must stop planning")
      } catch {
        #expect(error.status == status)
        #expect(error.code == "projectSettingsUnreadable")
      }
    }
    _ = try dir.file("jevision.project.json", #"{"format":1,"minVersion":"1.0.0+build.7","future":true}"#)
    #expect(try RunCommand.plan(request).tests == [test])
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
    #expect(throws: UsageError(message: "there is no environment “staging”: the project has no jevision.environments.json in \(dir.url.path)", code: "noEnvironmentsFile")) {
      try RunCommand.plan(RunRequest(paths: [login], environment: "staging"))
    }
    _ = try dir.file("jevision.environments.json", #"{"format":1,"environments":{"dev":{},"prod":{}}}"#)
    #expect(try RunCommand.plan(RunRequest(paths: [login], environment: "dev")).project == dir.url.path)
    #expect(throws: UsageError(message: "there is no environment “staging” in jevision.environments.json; use one of: dev, prod", code: "noEnvironment")) {
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
  /// What a look at the app's window sees; nil when it has none.
  private let screen: Screenshot?

  init(opening: DispatchSemaphore, isCancelled: @escaping @Sendable () -> Bool, screen: Screenshot? = nil) {
    self.opening = opening
    self.isCancelled = isCancelled
    self.screen = screen
  }

  func arrangeWindow(app: String, size: WindowSize?, position: WindowPosition?) throws(HandsError) {}

  func elements(app: String) throws(HandsError) -> [ElementNode] { [] }

  func openApp(app: String, start: AppStart, arguments: [String], environment: [String: String]) throws(HandsError) {
    opening.signal()
    while !isCancelled() { usleep(10_000) }
  }

  func sandboxed(app: String) throws(HandsError) -> Bool { false }
  func openItems(items: [Openable], app: String?, newInstance: Bool) throws(HandsError) {}
  func activate(app: String) throws(HandsError) {}
  func quit(app: String) throws(HandsError) {}
  func closeItems(items: [Openable], app: String?) throws(HandsError) {}
  func capture(app: String) throws(HandsError) -> Screenshot {
    guard let screen else { throw .NoWindow(app: app) }
    return screen
  }
  func locate(app: String, template: Data, minSimilarity: Double) throws(HandsError) -> Point? { nil }
  func click(app: String, at: Point, button: MouseButton, doubleClick: Bool, modifiers: Modifiers) throws(HandsError) {}
  func hover(app: String, at: Point, modifiers: Modifiers) throws(HandsError) {}
  func drag(app: String, path: [Point], button: MouseButton, modifiers: Modifiers) throws(HandsError) {}
  func scroll(
    app: String, at: Point, direction: ScrollDirection, amount: UInt32, unit: ScrollUnit, modifiers: Modifiers
  ) throws(HandsError) {}
  func typeText(text: String) throws(HandsError) {}
  func pressKeys(keys: String) throws(HandsError) {}
  func clipboard() throws(HandsError) -> String { "" }
  func setClipboard(text: String) throws(HandsError) {}
  func clipboardImage() throws(HandsError) -> Data? { nil }
  func clipboardFiles() throws(HandsError) -> [String] { [] }
  func setClipboardImage(png: Data) throws(HandsError) {}
  func userInputDetected() -> Bool { false }
  func startVideo(app: String, path: String, hide: [Rect]) throws(HandsError) {}
  func stopVideo() throws(HandsError) {}
}

private final class Lines: @unchecked Sendable {
  private let lock = NSLock()
  private var written: [String] = []

  func append(_ part: String) { lock.withLock { written.append(part) } }
  /// Each piece the console was given, as written.
  var parts: [String] { lock.withLock { written } }
  var text: String { lock.withLock { written.joined() } }
}

/// How many tests are on the screen at once.
private final class ScreenCounter: @unchecked Sendable {
  private let lock = NSLock()
  private var now = 0
  private(set) var most = 0
  private(set) var entered = 0

  func enter() {
    lock.withLock {
      now += 1
      entered += 1
      most = max(most, now)
    }
  }

  func leave() { lock.withLock { now -= 1 } }
}

/// Hands that take a moment to open the app and count the tests doing it together.
private final class CountingHands: Hands, @unchecked Sendable {
  private let counter: ScreenCounter

  init(counter: ScreenCounter) { self.counter = counter }

  func arrangeWindow(app: String, size: WindowSize?, position: WindowPosition?) throws(HandsError) {}

  func elements(app: String) throws(HandsError) -> [ElementNode] { [] }

  func openApp(app: String, start: AppStart, arguments: [String], environment: [String: String]) throws(HandsError) {
    counter.enter()
    usleep(250_000)
    counter.leave()
  }

  func sandboxed(app: String) throws(HandsError) -> Bool { false }
  func openItems(items: [Openable], app: String?, newInstance: Bool) throws(HandsError) {}
  func activate(app: String) throws(HandsError) {}
  func quit(app: String) throws(HandsError) {}
  func closeItems(items: [Openable], app: String?) throws(HandsError) {}
  func capture(app: String) throws(HandsError) -> Screenshot { throw .NoWindow(app: app) }
  func locate(app: String, template: Data, minSimilarity: Double) throws(HandsError) -> Point? { nil }
  func click(app: String, at: Point, button: MouseButton, doubleClick: Bool, modifiers: Modifiers) throws(HandsError) {}
  func hover(app: String, at: Point, modifiers: Modifiers) throws(HandsError) {}
  func drag(app: String, path: [Point], button: MouseButton, modifiers: Modifiers) throws(HandsError) {}
  func scroll(
    app: String, at: Point, direction: ScrollDirection, amount: UInt32, unit: ScrollUnit, modifiers: Modifiers
  ) throws(HandsError) {}
  func typeText(text: String) throws(HandsError) {}
  func pressKeys(keys: String) throws(HandsError) {}
  func clipboard() throws(HandsError) -> String { "" }
  func setClipboard(text: String) throws(HandsError) {}
  func clipboardImage() throws(HandsError) -> Data? { nil }
  func clipboardFiles() throws(HandsError) -> [String] { [] }
  func setClipboardImage(png: Data) throws(HandsError) {}
  func userInputDetected() -> Bool { false }
  func startVideo(app: String, path: String, hide: [Rect]) throws(HandsError) {}
  func stopVideo() throws(HandsError) {}
}

private final class Box<T>: @unchecked Sendable {
  var value: T?
}
