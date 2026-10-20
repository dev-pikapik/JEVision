import Foundation
import JEVisionCore
import JEVisionHands

/// `jevision`: runs, checks and formats JEVision tests from the command line.
@main
enum JEVisionCLI {
  static func main() {
    setLanguage(tag: CLILanguage.resolve(environment: ProcessInfo.processInfo.environment, preferred: Locale.preferredLanguages))
    exit(execute(Array(CommandLine.arguments.dropFirst())).rawValue)
  }

  /// Runs `arguments`. A `--lang` among them sets the language of what is printed from here on.
  static func execute(_ arguments: [String]) -> ExitStatus {
    let command: Command
    do {
      let (language, rest) = try CLILanguage.extract(arguments)
      if let language { setLanguage(tag: language) }
      command = try Command.parse(rest)
    } catch {
      Console.standardError.line("jevision: \(error.message)")
      Console.standardError.line(Help.usage(error.command))
      let more = error.command.map { "jevision \($0) --help" } ?? "jevision --help"
      Console.standardError.line(words("cli.help.more", more))
      return .failure
    }
    switch command {
    case .help(let topic):
      Console.standardOutput.line(Help.text(topic))
      return .ok
    case .version:
      Console.standardOutput.line("jevision \(jevisionVersion())")
      return .ok
    case .validate(let paths, let json):
      return validate(paths, json: json)
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
    }
  }
}

/// How a command ended, ordered by severity; the process exit status.
enum ExitStatus: Int32, Comparable {
  /// All good.
  case ok = 0
  /// Problems found in a test, or a test failed or was stopped.
  case problems = 1
  /// Wrong arguments, a file that could not be read or written, or a Mac not ready to run tests.
  case failure = 2

  static func < (a: Self, b: Self) -> Bool { a.rawValue < b.rawValue }
}

struct UsageError: Error, Equatable {
  let message: String
  /// The command whose usage to show.
  var command: String?
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

  /// What `--parallel` alone means: a test per core, the most the core allows at once.
  static var defaultParallel: UInt32 {
    min(max(1, UInt32(clamping: ProcessInfo.processInfo.activeProcessorCount)), maxParallelRuns())
  }
}

enum Command: Equatable {
  case help(String?)
  case version
  case validate(paths: [String], json: Bool)
  case fmt(paths: [String], check: Bool)
  case run(RunRequest)
  case mcp(project: String?, action: MCPAction)
  case scheduleRun(project: String)
  case issue(IssueRequest)
  case blocks(BlocksRequest)

  static func parse(_ arguments: [String]) throws(UsageError) -> Command {
    guard let name = arguments.first else { return .help(nil) }
    let rest = Array(arguments.dropFirst())
    switch name {
    case "-h", "--help", "help":
      guard let topic = rest.first else { return .help(nil) }
      guard Help.commands.contains(topic) else { throw UsageError(message: words("cli.error.noCommand", topic)) }
      return .help(topic)
    case "-V", "--version":
      return .version
    case "validate":
      let options = try Options.parse(rest, command: name, flags: ["--json"])
      if options.wantsHelp { return .help(name) }
      return .validate(paths: try options.requirePaths(name), json: options.flags.contains("--json"))
    case "fmt":
      let options = try Options.parse(rest, command: name, flags: ["--check"])
      if options.wantsHelp { return .help(name) }
      return .fmt(paths: try options.requirePaths(name), check: options.flags.contains("--check"))
    case "run":
      let options = try Options.parse(
        rest, command: name, flags: ["--json", "--include-quarantined-failures", "--no-notify", "--flaky"],
        values: [
          "--project", "--env", "--retries", "--row", "--rows", "--start-at", "--provider", "--model", "--junit", "--html", "--video",
          "--type", "--priority", "--status", "--author",
        ],
        optionalNumbers: ["--parallel"])
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
      let options = try Options.parse(rest, command: name, flags: ["--yes", "--force"], values: ["--project"])
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
    default:
      throw UsageError(message: name.hasPrefix("-") ? words("cli.error.noOption", name) : words("cli.error.noCommand", name))
    }
  }
}

extension Command {
  /// `blocks list|install <source>|update [id]|remove <id>`.
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
    default:
      throw UsageError(message: words("cli.error.noBlocksCommand", action), command: name)
    }
    var request = BlocksRequest(action: blocks, project: options.values["--project"])
    request.yes = options.flags.contains("--yes")
    request.force = options.flags.contains("--force")
    if request.yes, !["install", "update"].contains(action) {
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
  var wantsHelp = false

  static func parse(
    _ arguments: [String], command: String, flags: Set<String>, values: Set<String> = [],
    optionalNumbers: Set<String> = []
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
      guard values.contains(name) else {
        throw UsageError(message: words("cli.error.noOption", argument), command: command)
      }
      guard options.values[name] == nil else { throw UsageError(message: words("cli.error.optionTwice", name), command: command) }
      guard let value = parts.count == 2 ? parts[1] : rest.popFirst(), !value.isEmpty else {
        throw UsageError(message: words("cli.error.optionNeedsValue", name), command: command)
      }
      options.values[name] = value
    }
    return options
  }

  func requirePaths(_ command: String) throws(UsageError) -> [String] {
    guard !paths.isEmpty else { throw UsageError(message: words("cli.error.noPaths"), command: command) }
    return paths
  }
}

enum Help {
  static let commands = ["run", "validate", "fmt", "issue", "mcp", "schedule", "blocks"]

  static func usage(_ command: String?) -> String {
    switch command {
    case "run": words("cli.usage.run")
    case "validate": words("cli.usage.validate")
    case "fmt": words("cli.usage.fmt")
    case "issue": words("cli.usage.issue")
    case "mcp": words("cli.usage.mcp")
    case "schedule": words("cli.usage.schedule")
    case "blocks": words("cli.usage.blocks")
    default: words("cli.usage.main")
    }
  }

  static func text(_ command: String?) -> String {
    let usage = usage(command)
    return switch command {
    case "run": words("cli.help.run", usage)
    case "validate": words("cli.help.validate", usage)
    case "fmt": words("cli.help.fmt", usage)
    case "issue": words("cli.help.issue", usage)
    case "mcp": words("cli.help.mcp", usage)
    case "blocks": words("cli.help.blocks", usage)
    case "schedule": words("cli.help.schedule", usage)
    default: words("cli.help.main", usage, catalogLanguages().joined(separator: ", "))
    }
  }
}
