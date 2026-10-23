import Foundation
import JEVisionCore
import JEVisionHands

/// `jevision`: runs, checks and formats JEVision tests from the command line.
@main
enum JEVisionCLI {
  static func main() {
    CLILanguage.apply(CLILanguage.resolve(environment: ProcessInfo.processInfo.environment, preferred: Locale.preferredLanguages))
    exit(execute(Array(CommandLine.arguments.dropFirst())))
  }

  /// Runs `arguments` and returns the exit status. A `--lang` among them sets the language of
  /// what is printed from here on.
  static func execute(_ arguments: [String]) -> Int32 {
    let command: Command
    do {
      let (language, rest) = try CLILanguage.extract(arguments)
      if let language { CLILanguage.apply(language) }
      command = try Command.parse(rest)
    } catch {
      Console.standardError.line("jevision: \(error.message)")
      if arguments.contains("--json") { Console.standardOutput.line(JSON.result([("error", error.json)]).pretty) }
      Console.standardError.line(Help.usage(error.command))
      let more = error.command.map { "jevision \($0) --help" } ?? "jevision --help"
      Console.standardError.line(words("cli.help.more", more))
      return error.status.rawValue
    }
    if case .run(let request) = command, request.throughApp {
      let app: URL
      do {
        app = try ThroughApp.findApp(
          executable: Bundle.main.executableURL, given: request.app, environment: ProcessInfo.processInfo.environment,
          home: FileManager.default.homeDirectoryForCurrentUser, current: FileManager.default.currentDirectoryPath)
      } catch {
        Console.standardError.line("jevision: \(error.message)")
        return error.status.rawValue
      }
      return ThroughApp(arguments: ThroughApp.forwarded(arguments), app: app).execute()
    }
    return perform(command).rawValue
  }

  private static func perform(_ command: Command) -> ExitStatus {
    switch command {
    case .help(let topic):
      Console.standardOutput.line(Help.text(topic))
      return .ok
    case .version(let json):
      Console.standardOutput.line(json ? versionJSON().pretty : "jevision \(jevisionVersion())")
      return .ok
    case .validate(let paths, let json):
      return validate(paths, json: json)
    case .check(let project, let json):
      return CheckCommand(project: project, json: json).execute()
    case .fmt(let paths, let check):
      return format(paths, check: check)
    case .run(let request):
      return RunCommand(request: request).execute()
    case .mcp(let project, let action):
      return MCPCommand(project: project, action: action).execute()
    case .scheduleRun(let project):
      return ScheduleCommand(project: project).execute()
    case .issue(let request):
      return IssueCommand(request: request).execute()
    case .blocks(let request):
      return BlocksCommand(request: request).execute()
    case .migrate(let request):
      return MigrateCommand(request: request).execute()
    }
  }

  /// `version --json`: the version and what it reads and speaks.
  static func versionJSON() -> JSON {
    let about = compatibility()
    return .result([
      ("version", .string(jevisionVersion())),
      ("formats", .array(about.formats.map { .object([("file", .string($0.file)), ("version", .int(Int64($0.version)))]) })),
      ("iphoneProtocol", .object([
        ("oldest", .int(Int64(about.iphoneProtocolOldest))), ("newest", .int(Int64(about.iphoneProtocolNewest))),
      ])),
      ("mcpProtocols", .array(about.mcpProtocols.map { .string($0) })),
    ])
  }
}

/// How a command ended, ordered by severity; the process exit status. The same for every
/// command, so scripts can tell why a command failed. 6 to 63 are kept for later.
enum ExitStatus: Int32, Comparable {
  /// All good.
  case ok = 0
  /// A test failed or was stopped, or problems found in a test or a project.
  case problems = 1
  /// Wrong arguments, or a file, folder or test that isn't there.
  case usage = 2
  /// A file couldn't be read or written.
  case io = 3
  /// This Mac can't run tests now, or a service it needs is out of reach.
  case notReady = 4
  /// A file was saved by a newer JEVision.
  case needsNewer = 5

  static func < (a: Self, b: Self) -> Bool { a.rawValue < b.rawValue }

  /// `.needsNewer` for a file a newer JEVision saved, `.io` for any other file that can't be read.
  static func unreadable(_ path: String) -> Self {
    needsNewerJevision(path: path) ? .needsNewer : .io
  }
}

struct UsageError: Error, Equatable {
  let message: String
  /// Names the error in `--json` output, the same in every language.
  var code = "usage"
  /// The command whose usage to show.
  var command: String?
  /// The exit status, when the arguments are right but what they name can't be used.
  var status: ExitStatus = .usage

  /// The message without the `jevision: ` it starts with on standard error.
  var reason: String { message.hasPrefix("jevision: ") ? String(message.dropFirst(10)) : message }

  /// The error as `--json` output tells it.
  var json: JSON { .object([("code", .string(code)), ("message", .string(reason))]) }
}

/// What `jevision run` was asked to do.
struct RunRequest: Equatable {
  var paths: [String] = []
  var project: String?
  var retries: UInt32?
  /// Rows of the tests' data tables to run, counted from 0; `nil` runs all of them.
  var rows: DataRowRange?
  /// Environment of the project whose values `${env.NAME}` reads.
  var environment: String?
  var startAt: String?
  /// AI provider and model for checks by meaning, instead of the ones chosen in the app.
  var provider: String?
  var model: String?
  /// Every check by meaning asks the run's provider and model, whatever the project or a test
  /// chooses: for CI, which has the key of one provider.
  var forceAI = false
  var junit: String?
  var html: String?
  /// Whether to record a video of the app; `nil` follows the app's setting.
  var video: VideoMode?
  /// Name of the schedule that started the run, saved in its report.
  var schedule: String?
  /// Failures of tests in quarantine count toward the exit status and the JUnit report.
  var includeQuarantinedFailures = false
  /// Which of the named tests to run, by the fields of their cards; asks nothing by default.
  var filter = TestFilter()
  /// Send the messages jevision.notifications.json asks for after each test.
  var notify = true
  var json = false
  /// Most tests to run side by side; `nil` runs them one after another.
  var parallel: UInt32?
  /// Copies of apps to use instead of the installed ones: absolute paths by the app id they
  /// stand for.
  var appCopies: [String: String] = [:]
  /// Run inside JEVision.app, the one at `app` if given, rather than in this process.
  var throughApp = false
  var app: String?

  /// What `--parallel` alone means: a test per core, the most the core allows at once.
  static var defaultParallel: UInt32 {
    min(max(1, UInt32(clamping: ProcessInfo.processInfo.activeProcessorCount)), maxParallelRuns())
  }
}

enum Command: Equatable {
  case help(String?)
  case version(json: Bool)
  case validate(paths: [String], json: Bool)
  case check(project: String?, json: Bool)
  case fmt(paths: [String], check: Bool)
  case run(RunRequest)
  case mcp(project: String?, action: MCPAction)
  case scheduleRun(project: String)
  case issue(IssueRequest)
  case blocks(BlocksRequest)
  case migrate(MigrateRequest)

  static func parse(_ arguments: [String]) throws(UsageError) -> Command {
    guard let name = arguments.first else { return .help(nil) }
    let rest = Array(arguments.dropFirst())
    switch name {
    case "-h", "--help", "help":
      guard let topic = rest.first else { return .help(nil) }
      guard Help.commands.contains(topic) else { throw UsageError(message: words("cli.error.noCommand", topic)) }
      return .help(topic)
    case "-V", "--version":
      return .version(json: false)
    case "version":
      let options = try Options.parse(rest, command: name, flags: ["--json"])
      if options.wantsHelp { return .help(name) }
      guard options.paths.isEmpty else { throw UsageError(message: words("cli.error.versionNoArguments"), command: name) }
      return .version(json: options.flags.contains("--json"))
    case "validate":
      let options = try Options.parse(rest, command: name, flags: ["--json"])
      if options.wantsHelp { return .help(name) }
      return .validate(paths: try options.requirePaths(name), json: options.flags.contains("--json"))
    case "check":
      let options = try Options.parse(rest, command: name, flags: ["--json"])
      if options.wantsHelp { return .help(name) }
      guard options.paths.count <= 1 else { throw UsageError(message: words("cli.error.oneProject"), command: name) }
      return .check(project: options.paths.first, json: options.flags.contains("--json"))
    case "fmt":
      let options = try Options.parse(rest, command: name, flags: ["--check"])
      if options.wantsHelp { return .help(name) }
      return .fmt(paths: try options.requirePaths(name), check: options.flags.contains("--check"))
    case "run":
      let options = try Options.parse(
        rest, command: name, flags: ["--json", "--include-quarantined-failures", "--no-notify", "--flaky", "--force-ai", "--through-app"],
        values: [
          "--app", "--project", "--env", "--retries", "--row", "--rows", "--start-at", "--provider", "--model", "--junit", "--html", "--video",
          "--type", "--priority", "--status", "--author",
        ],
        optionalNumbers: ["--parallel"], repeated: ["--app-copy"])
      if options.wantsHelp { return .help(name) }
      var request = RunRequest(paths: try options.requirePaths(name))
      request.project = options.values["--project"]
      if let retries = options.values["--retries"] {
        guard let count = UInt32(retries) else {
          throw UsageError(message: words("cli.error.retriesNumber", retries), command: name)
        }
        request.retries = count
      }
      if options.values["--row"] != nil, options.values["--rows"] != nil {
        throw UsageError(message: words("cli.error.eitherOr", "--row", "--rows"), command: name)
      }
      if let rows = options.values["--rows"] ?? options.values["--row"] {
        do {
          request.rows = try parseDataRows(text: rows)
        } catch DataRowsChoiceError.Invalid(let message) {
          throw UsageError(message: message, command: name)
        } catch {
          throw UsageError(message: "\(error)", command: name)
        }
      }
      request.environment = options.values["--env"]
      request.startAt = options.values["--start-at"]
      if request.startAt != nil, request.paths.count > 1 || isFolder(request.paths[0]) {
        throw UsageError(message: words("cli.error.startAtOneTest"), command: name)
      }
      request.provider = options.values["--provider"]
      if let provider = request.provider, AISettings.provider(provider) == nil {
        let ids = AISettings.providers.map(\.id).joined(separator: ", ")
        throw UsageError(message: words("cli.error.noProvider", provider, ids), command: name)
      }
      request.model = options.values["--model"]
      request.forceAI = options.flags.contains("--force-ai")
      request.junit = options.values["--junit"]
      request.html = options.values["--html"]
      if let video = options.values["--video"] {
        guard let mode = VideoMode(name: video) else {
          let names = VideoMode.all.map(\.name).joined(separator: ", ")
          throw UsageError(message: words("cli.error.videoMode", names, video), command: name)
        }
        request.video = mode
      }
      request.includeQuarantinedFailures = options.flags.contains("--include-quarantined-failures")
      do {
        request.filter = try parseTestFilter(
          testType: options.values["--type"], priority: options.values["--priority"], status: options.values["--status"],
          author: options.values["--author"], flaky: options.flags.contains("--flaky"))
      } catch TestFilterError.Invalid(let reason) {
        throw UsageError(message: reason, command: name)
      } catch {
        throw UsageError(message: "\(error)", command: name)
      }
      request.notify = !options.flags.contains("--no-notify")
      request.json = options.flags.contains("--json")
      request.appCopies = try appCopies(options.lists["--app-copy"] ?? [])
      request.throughApp = options.flags.contains("--through-app")
      request.app = options.values["--app"]
      if request.app != nil, !request.throughApp {
        throw UsageError(message: words("cli.error.appNeedsThroughApp"), command: name)
      }
      if options.flags.contains("--parallel") {
        guard let given = options.values["--parallel"] else {
          request.parallel = RunRequest.defaultParallel
          return .run(request)
        }
        let most = maxParallelRuns()
        guard let limit = UInt32(given), (1...most).contains(limit) else {
          throw UsageError(message: words("cli.error.parallelNumber", most, given), command: name)
        }
        request.parallel = limit
      }
      return .run(request)
    case "mcp":
      let options = try Options.parse(rest, command: name, flags: [], values: ["--project", "--pair", "--remote", "--forget"])
      if options.wantsHelp { return .help(name) }
      if let extra = options.paths.first {
        throw UsageError(message: words("cli.error.mcpNoPaths", extra), command: name)
      }
      let actions = ["--pair", "--remote", "--forget"].filter { options.values[$0] != nil }
      guard actions.count <= 1 else {
        throw UsageError(message: words("cli.error.oneAtATime", actions.joined(separator: ", ")), command: name)
      }
      guard let option = actions.first, let value = options.values[option] else {
        return .mcp(project: options.values["--project"], action: .serve)
      }
      if value.trimmingCharacters(in: .whitespaces).isEmpty {
        throw UsageError(
          message: option == "--pair" ? words("cli.error.pairNeedsCode") : words("cli.error.optionNeedsMac", option), command: name)
      }
      if option != "--remote", options.values["--project"] != nil {
        throw UsageError(message: words("cli.error.projectWithRemote", option), command: name)
      }
      let action: MCPAction =
        switch option {
        case "--pair": .pair(value)
        case "--remote": .remote(value)
        default: .forget(value)
        }
      return .mcp(project: options.values["--project"], action: action)
    case "schedule":
      let options = try Options.parse(rest, command: name, flags: [])
      if options.wantsHelp || options.paths.isEmpty { return .help(name) }
      guard options.paths[0] == "run" else {
        throw UsageError(message: words("cli.error.noScheduleCommand", options.paths[0]), command: name)
      }
      guard options.paths.count == 2 else {
        throw UsageError(message: words("cli.error.oneProject"), command: name)
      }
      return .scheduleRun(project: options.paths[1])
    case "blocks":
      let options = try Options.parse(rest, command: name, flags: ["--yes", "--force"], values: ["--project", "--only"])
      if options.wantsHelp || options.paths.isEmpty { return .help(name) }
      return .blocks(try blocksRequest(options))
    case "issue":
      let options = try Options.parse(
        rest, command: name, flags: ["--send", "--new"], values: ["--project", "--title", "--comment-on"])
      if options.wantsHelp { return .help(name) }
      guard options.paths.count <= 1 else { throw UsageError(message: words("cli.error.oneTestOrRun"), command: name) }
      var request = IssueRequest(target: options.paths.first, project: options.values["--project"])
      request.send = options.flags.contains("--send")
      request.title = options.values["--title"]
      request.newIssue = options.flags.contains("--new")
      if let number = options.values["--comment-on"] {
        guard let issue = UInt64(number.drop { $0 == "#" }) else {
          throw UsageError(message: words("cli.error.commentOnNumber", number), command: name)
        }
        request.commentOn = issue
      }
      if request.commentOn != nil, request.newIssue {
        throw UsageError(message: words("cli.error.eitherOr", "--comment-on", "--new"), command: name)
      }
      if !request.send, request.commentOn != nil || request.newIssue {
        throw UsageError(message: words("cli.error.sendMissing"), command: name)
      }
      return .issue(request)
    case "migrate":
      let options = try Options.parse(rest, command: name, flags: ["--dry-run", "--backups", "--json"], values: ["--restore"])
      if options.wantsHelp { return .help(name) }
      return .migrate(try migrateRequest(options))
    default:
      throw UsageError(message: name.hasPrefix("-") ? words("cli.error.noOption", name) : words("cli.error.noCommand", name))
    }
  }
}

extension Command {
  /// `migrate [--dry-run | --backups | --restore <id>] [project-folder]`.
  private static func migrateRequest(_ options: Options) throws(UsageError) -> MigrateRequest {
    let name = "migrate"
    let actions = ["--dry-run", "--backups"].filter(options.flags.contains) + options.values.keys.filter { $0 == "--restore" }
    guard actions.count <= 1 else {
      throw UsageError(message: words("cli.error.oneAtATime", actions.joined(separator: ", ")), command: name)
    }
    guard options.paths.count <= 1 else { throw UsageError(message: words("cli.error.oneProject"), command: name) }
    let action: MigrateRequest.Action =
      if let id = options.values["--restore"] {
        .restore(id: id)
      } else if options.flags.contains("--backups") {
        .backups
      } else {
        .migrate(dryRun: options.flags.contains("--dry-run"))
      }
    return MigrateRequest(action: action, project: options.paths.first ?? ".", json: options.flags.contains("--json"))
  }

  /// `blocks list|install <source>|update [id]|remove <id>|restore`.
  /// The copies `--app-copy ID=PATH` names: absolute paths by app id, a relative one taken from
  /// `current`.
  static func appCopies(
    _ values: [String], current: String = FileManager.default.currentDirectoryPath
  ) throws(UsageError) -> [String: String] {
    var copies: [String: String] = [:]
    for value in values {
      let parts = value.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false).map(String.init)
      guard parts.count == 2, !parts[0].isEmpty, !parts[1].isEmpty else {
        throw UsageError(message: words("cli.error.appCopyFormat", value), command: "run")
      }
      let id = parts[0]
      let path = absolutePath(parts[1], in: current)
      guard copies[id] == nil else { throw UsageError(message: words("cli.error.appCopyTwice", id), command: "run") }
      guard (path as NSString).pathExtension == "app" else {
        throw UsageError(message: words("cli.error.appCopyNotApp", parts[1]), command: "run")
      }
      guard isFolder(path) else { throw UsageError(message: words("cli.error.appCopyMissing", path), command: "run") }
      copies[id] = path
    }
    return copies
  }

  private static func blocksRequest(_ options: Options) throws(UsageError) -> BlocksRequest {
    let name = "blocks"
    let (action, arguments) = (options.paths[0], Array(options.paths.dropFirst()))
    func argument(_ missing: String) throws(UsageError) -> String {
      guard arguments.count == 1 else { throw UsageError(message: missing, command: name) }
      return arguments[0]
    }
    let blocks: BlocksRequest.Action
    switch action {
    case "list":
      guard arguments.isEmpty else { throw UsageError(message: words("cli.error.listNoArguments"), command: name) }
      blocks = .list
    case "install": blocks = .install(source: try argument(words("cli.error.oneBlockSource")))
    case "remove": blocks = .remove(id: try argument(words("cli.error.oneBlockId")))
    case "update":
      guard arguments.count <= 1 else { throw UsageError(message: words("cli.error.oneBlockIdOrNone"), command: name) }
      blocks = .update(id: arguments.first)
    case "restore":
      guard arguments.isEmpty else { throw UsageError(message: words("cli.error.restoreNoArguments"), command: name) }
      blocks = .restore
    default:
      throw UsageError(message: words("cli.error.noBlocksCommand", action), command: name)
    }
    var request = BlocksRequest(action: blocks, project: options.values["--project"])
    request.yes = options.flags.contains("--yes")
    request.force = options.flags.contains("--force")
    if let only = options.values["--only"] {
      request.only = only.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
      guard !request.only.isEmpty else { throw UsageError(message: words("cli.error.onlyNeedsIds"), command: name) }
      guard ["install", "restore"].contains(action) else {
        throw UsageError(message: words("cli.error.onlyBelongs"), command: name)
      }
    }
    if request.yes, !["install", "update", "restore"].contains(action) {
      throw UsageError(message: words("cli.error.yesBelongs"), command: name)
    }
    if request.force, action == "list" {
      throw UsageError(message: words("cli.error.forceBelongs"), command: name)
    }
    return request
  }
}

/// Options and paths after the command name: `--flag`, `--name value`, `--name=value`;
/// everything after `--` is a path. An option in `optionalNumbers` may stand alone or take a
/// number (`--parallel`, `--parallel 4`, `--parallel=4`); it is in `flags`, and its number in `values`.
struct Options {
  var paths: [String] = []
  var flags: Set<String> = []
  var values: [String: String] = [:]
  /// Values of the options that may be given more than once, in order.
  var lists: [String: [String]] = [:]
  var wantsHelp = false

  static func parse(
    _ arguments: [String], command: String, flags: Set<String>, values: Set<String> = [],
    optionalNumbers: Set<String> = [], repeated: Set<String> = []
  ) throws(UsageError) -> Options {
    var options = Options()
    var rest = arguments[...]
    var onlyPaths = false
    while let argument = rest.popFirst() {
      if onlyPaths || !argument.hasPrefix("-") || argument == "-" {
        options.paths.append(argument)
        continue
      }
      if argument == "--" {
        onlyPaths = true
        continue
      }
      if argument == "-h" || argument == "--help" {
        options.wantsHelp = true
        continue
      }
      let parts = argument.split(separator: "=", maxSplits: 1).map(String.init)
      let name = parts[0]
      if flags.contains(name) {
        guard parts.count == 1 else { throw UsageError(message: words("cli.error.optionTakesNoValue", name), command: command) }
        options.flags.insert(name)
        continue
      }
      if optionalNumbers.contains(name) {
        guard options.flags.insert(name).inserted else {
          throw UsageError(message: words("cli.error.optionTwice", name), command: command)
        }
        if parts.count == 2 {
          options.values[name] = parts[1]
        } else if let next = rest.first, next.allSatisfy({ $0 >= "0" && $0 <= "9" }) {
          options.values[name] = rest.removeFirst()
        }
        continue
      }
      guard values.contains(name) || repeated.contains(name) else {
        throw UsageError(message: words("cli.error.noOption", argument), command: command)
      }
      guard options.values[name] == nil else { throw UsageError(message: words("cli.error.optionTwice", name), command: command) }
      guard let value = parts.count == 2 ? parts[1] : rest.popFirst(), !value.isEmpty else {
        throw UsageError(message: words("cli.error.optionNeedsValue", name), command: command)
      }
      if repeated.contains(name) {
        options.lists[name, default: []].append(value)
      } else {
        options.values[name] = value
      }
    }
    return options
  }

  func requirePaths(_ command: String) throws(UsageError) -> [String] {
    guard !paths.isEmpty else { throw UsageError(message: words("cli.error.noPaths"), command: command) }
    return paths
  }
}

enum Help {
  static let commands = ["run", "validate", "check", "fmt", "issue", "mcp", "schedule", "blocks", "version", "migrate"]

  static func usage(_ command: String?) -> String {
    switch command {
    case "run": words("cli.usage.run")
    case "validate": words("cli.usage.validate")
    case "check": words("cli.usage.check")
    case "fmt": words("cli.usage.fmt")
    case "issue": words("cli.usage.issue")
    case "mcp": words("cli.usage.mcp")
    case "schedule": words("cli.usage.schedule")
    case "blocks": words("cli.usage.blocks")
    case "version": words("cli.usage.version")
    case "migrate": words("cli.usage.migrate")
    default: words("cli.usage.main")
    }
  }

  static func text(_ command: String?) -> String {
    let usage = usage(command)
    return switch command {
    case "run": words("cli.help.run", usage)
    case "validate": words("cli.help.validate", usage)
    case "check": words("cli.help.check", usage)
    case "fmt": words("cli.help.fmt", usage)
    case "issue": words("cli.help.issue", usage)
    case "mcp": words("cli.help.mcp", usage)
    case "blocks": words("cli.help.blocks", usage)
    case "schedule": words("cli.help.schedule", usage)
    case "version": words("cli.help.version", usage)
    case "migrate": words("cli.help.migrate", usage)
    default: words("cli.help.main", usage, catalogLanguages().joined(separator: ", "))
    }
  }
}
