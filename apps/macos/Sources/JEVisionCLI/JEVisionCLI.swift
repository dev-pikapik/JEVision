import Foundation
import JEVisionCore
import JEVisionHands

/// `jevision`: runs, checks and formats JEVision tests from the command line.
@main
enum JEVisionCLI {
  static func main() {
    exit(execute(Array(CommandLine.arguments.dropFirst())).rawValue)
  }

  static func execute(_ arguments: [String]) -> ExitStatus {
    let command: Command
    do {
      command = try Command.parse(arguments)
    } catch {
      Console.standardError.line("jevision: \(error.message)")
      Console.standardError.line(Help.usage(error.command))
      let more = error.command.map { "jevision \($0) --help" } ?? "jevision --help"
      Console.standardError.line("Run “\(more)” for more.")
      return .failure
    }
    switch command {
    case .help(let topic):
      Console.standardOutput.write(Help.text(topic))
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
    case .mcp(let project):
      return MCPCommand(project: project).execute()
    case .scheduleRun(let project):
      return ScheduleCommand(project: project).execute()
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
  var json = false
}

enum Command: Equatable {
  case help(String?)
  case version
  case validate(paths: [String], json: Bool)
  case fmt(paths: [String], check: Bool)
  case run(RunRequest)
  case mcp(project: String?)
  case scheduleRun(project: String)

  static func parse(_ arguments: [String]) throws(UsageError) -> Command {
    guard let name = arguments.first else { return .help(nil) }
    let rest = Array(arguments.dropFirst())
    switch name {
    case "-h", "--help", "help":
      guard let topic = rest.first else { return .help(nil) }
      guard Help.commands.contains(topic) else { throw UsageError(message: "there is no command “\(topic)”") }
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
        rest, command: name, flags: ["--json", "--include-quarantined-failures"],
        values: ["--project", "--env", "--retries", "--row", "--rows", "--start-at", "--provider", "--model", "--junit", "--html", "--video"])
      if options.wantsHelp { return .help(name) }
      var request = RunRequest(paths: try options.requirePaths(name))
      request.project = options.values["--project"]
      if let retries = options.values["--retries"] {
        guard let count = UInt32(retries) else {
          throw UsageError(message: "--retries needs a whole number of times, such as 2, not “\(retries)”", command: name)
        }
        request.retries = count
      }
      if options.values["--row"] != nil, options.values["--rows"] != nil {
        throw UsageError(message: "use either --row or --rows, not both", command: name)
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
        throw UsageError(message: "--start-at works with one test file only", command: name)
      }
      request.provider = options.values["--provider"]
      if let provider = request.provider, AISettings.provider(provider) == nil {
        let ids = AISettings.providers.map(\.id).joined(separator: ", ")
        throw UsageError(message: "there is no AI provider “\(provider)”; use one of: \(ids)", command: name)
      }
      request.model = options.values["--model"]
      request.junit = options.values["--junit"]
      request.html = options.values["--html"]
      if let video = options.values["--video"] {
        guard let mode = VideoMode(name: video) else {
          let names = VideoMode.all.map(\.name).joined(separator: ", ")
          throw UsageError(message: "--video needs one of: \(names), not “\(video)”", command: name)
        }
        request.video = mode
      }
      request.includeQuarantinedFailures = options.flags.contains("--include-quarantined-failures")
      request.json = options.flags.contains("--json")
      return .run(request)
    case "mcp":
      let options = try Options.parse(rest, command: name, flags: [], values: ["--project"])
      if options.wantsHelp { return .help(name) }
      if let extra = options.paths.first {
        throw UsageError(message: "mcp takes no paths; name the project folder with --project, not “\(extra)”", command: name)
      }
      return .mcp(project: options.values["--project"])
    case "schedule":
      let options = try Options.parse(rest, command: name, flags: [])
      if options.wantsHelp || options.paths.isEmpty { return .help(name) }
      guard options.paths[0] == "run" else {
        throw UsageError(message: "there is no schedule command “\(options.paths[0])”", command: name)
      }
      guard options.paths.count == 2 else {
        throw UsageError(message: "name one project folder", command: name)
      }
      return .scheduleRun(project: options.paths[1])
    default:
      let what = name.hasPrefix("-") ? "option" : "command"
      throw UsageError(message: "there is no \(what) “\(name)”")
    }
  }
}

/// Options and paths after the command name: `--flag`, `--name value`, `--name=value`;
/// everything after `--` is a path.
struct Options {
  var paths: [String] = []
  var flags: Set<String> = []
  var values: [String: String] = [:]
  var wantsHelp = false

  static func parse(
    _ arguments: [String], command: String, flags: Set<String>, values: Set<String> = []
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
        guard parts.count == 1 else { throw UsageError(message: "\(name) takes no value", command: command) }
        options.flags.insert(name)
        continue
      }
      guard values.contains(name) else {
        throw UsageError(message: "there is no option “\(argument)”", command: command)
      }
      guard options.values[name] == nil else { throw UsageError(message: "\(name) is given twice", command: command) }
      guard let value = parts.count == 2 ? parts[1] : rest.popFirst(), !value.isEmpty else {
        throw UsageError(message: "\(name) needs a value", command: command)
      }
      options.values[name] = value
    }
    return options
  }

  func requirePaths(_ command: String) throws(UsageError) -> [String] {
    guard !paths.isEmpty else { throw UsageError(message: "name at least one test file or folder", command: command) }
    return paths
  }
}

enum Help {
  static let commands = ["run", "validate", "fmt", "mcp", "schedule"]

  static func usage(_ command: String?) -> String {
    switch command {
    case "run": "Usage: jevision run <tests>… [options]"
    case "validate": "Usage: jevision validate <tests>… [--json]"
    case "fmt": "Usage: jevision fmt <tests>… [--check]"
    case "mcp": "Usage: jevision mcp [--project <folder>]"
    case "schedule": "Usage: jevision schedule run <project>"
    default: "Usage: jevision <command> [options]"
    }
  }

  static func text(_ command: String?) -> String {
    switch command {
    case "run":
      """
      Runs tests on this Mac, one after another, and saves a report of every run.
      The tests a test runs after ("dependsOn") run before it, even when they aren't
      named. When one of them doesn't pass, the tests that need it are blocked: they
      don't run, and their reports say why.

      \(usage(command))

      Arguments:
        <tests>…            Test files (.flow.json) or folders to search for them

      Options:
        --project <folder>  Project folder. Files the tests use must be inside it, and
                            reports are saved in <folder>/.jevision/runs. Default: the
                            nearest folder above the tests that has a .jevision folder,
                            otherwise the current folder if it holds the tests
        --env <name>        Environment from jevision.environments.json in the project
                            folder whose values the tests read as ${env.NAME}, such as
                            staging. Secrets come from the Keychain items of that
                            environment first. Default: the "default" environment
                            named in that file, if any
        --retries <n>       Run a failed test up to <n> more times (instead of the
                            number in the test, or the project's default from
                            jevision.project.json)
        --row <n>           Run a data-driven test with row <n> of its CSV table only,
                            counting from 1 (the header line isn't a row)
        --rows <which>      Rows of a data-driven test's CSV table to run: all (the
                            default), one row such as 3, or a range such as 2-4. Each
                            row runs the whole test and has its own retries
        --start-at <block>  Start from this block instead of the first one (one test only)
        --provider <id>     AI provider for checks by meaning instead of the one chosen
                            in the app (Settings → AI): anthropic, openai, gemini,
                            ollama, lmstudio or openai-compatible
        --model <name>      AI model for checks by meaning instead of the chosen one
        --junit <file>      Also save a JUnit XML report of all the tests, for CI
        --html <folder>     Also save the HTML report of every run in this folder
        --video <when>      Record a video of the app: off, on-failure (keep it only
                            when the test fails) or always. Default: as chosen in the
                            app (Settings → Advanced), otherwise on-failure
        --include-quarantined-failures
                            Count failures of tests in quarantine (listed in
                            jevision.project.json) like any other failure. Without it
                            they still run, but don't change the exit status and are
                            skipped in the JUnit report
        --json              Print the result as JSON; progress goes to standard error
        -h, --help          Show this help

      While tests run, JEVision uses the mouse and keyboard. Press Esc twice or Ctrl-C
      to stop. Running needs a logged-in user with an unlocked screen, and Screen &
      System Audio Recording and Accessibility (Device Control and Data Access from
      macOS 27) allowed for the app jevision runs in (Terminal or your CI agent).
      Secrets (${secret.NAME}) come from environment variables or the Keychain. So do
      API keys of AI providers, such as ANTHROPIC_API_KEY.

      Exit status: 0 — all tests passed (tests in quarantine may have failed), 1 — a
      test failed or was stopped, 2 — wrong arguments, a file could not be read or written, or this Mac can't run tests now.

      """
    case "validate":
      """
      Checks test files for errors and warnings.

      \(usage(command))

      Arguments:
        <tests>…    Test files (.flow.json) or folders to search for them

      Options:
        --json      Print the result as JSON
        -h, --help  Show this help

      Exit status: 0 — all good, 1 — problems found, 2 — wrong arguments or a file could
      not be read.

      """
    case "fmt":
      """
      Rewrites test files in canonical form: fixed key order, blocks in reading order,
      settings that are off or empty left out, and the click button, comparison and
      wait timeout always written, so every change shows up as a small, readable diff.

      \(usage(command))

      Arguments:
        <tests>…    Test files (.flow.json) or folders to search for them

      Options:
        --check     Only list the files that are not in canonical form; change nothing
        -h, --help  Show this help

      Exit status: 0 — all good, 1 — a file is not in canonical form or can't be read as
      a test, 2 — wrong arguments or a file could not be read or written.

      """
    case "mcp":
      """
      Serves the tests of a project to AI assistants on this Mac, such as Claude, Cursor or
      Copilot, over the Model Context Protocol. The assistant starts this command itself and
      talks to it over standard input and output; add it to the assistant's MCP servers.

      \(usage(command))

      Options:
        --project <folder>  Project folder whose tests the assistant may see. Default: the
                            nearest folder at or above the current one that has a .jevision
                            folder, otherwise the current folder
        -h, --help          Show this help

      Assistants can list, read, write and check tests, read reports and see the test
      format. Running tests and taking screenshots work only after you allow them in
      JEVision → Settings → AI → Assistants. Assistants never get the values of secrets
      or API keys, and can't touch files outside the project folder.

      """
    case "schedule":
      """
      Runs the schedules of a project that are due now, from jevision.schedule.json in the
      project folder. The JEVision app sets up macOS to start this command at the times of
      the schedules you make in it (Test → Schedules…); you don’t need to run it yourself.

      \(usage(command))

      Arguments:
        <project>   Project folder

      Options:
        -h, --help  Show this help

      A schedule is due once for its latest time since the last check, even when the Mac
      slept through it, unless that time is more than 12 hours ago. When the screen is
      locked, nobody is logged in or macOS hasn’t allowed jevision to record the screen
      and control the computer, the run is skipped and nothing is clicked or typed. What
      came of every scheduled run is kept in .jevision/schedule-history.json, and reports
      of the tests are saved like those of jevision run.

      Exit status: 0 — nothing was due or all tests passed, 1 — a test failed or a run was
      skipped, 2 — the project or its schedule file couldn’t be read, or a run couldn’t start.

      """
    default:
      """
      Runs, checks and formats JEVision tests.

      \(usage(nil))

      Commands:
        run        Run tests on this Mac
        validate   Check tests for errors and warnings
        fmt        Rewrite tests in canonical form
        mcp        Let AI assistants read, edit and run tests (MCP server)
        schedule   Run the schedules of a project that are due now

      Options:
        -h, --help     Show this help
        -V, --version  Show the version

      Run “jevision <command> --help” for the options of a command.

      """
    }
  }
}
