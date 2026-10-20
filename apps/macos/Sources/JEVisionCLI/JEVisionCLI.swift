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
          throw UsageError(message: "--parallel needs a number of tests from 1 to \(most), such as 4, not “\(given)”", command: name)
        }
        request.parallel = limit
      }
      return .run(request)
    case "mcp":
      let options = try Options.parse(rest, command: name, flags: [], values: ["--project", "--pair", "--remote", "--forget"])
      if options.wantsHelp { return .help(name) }
      if let extra = options.paths.first {
        throw UsageError(message: "mcp takes no paths; name the project folder with --project, not “\(extra)”", command: name)
      }
      let actions = ["--pair", "--remote", "--forget"].filter { options.values[$0] != nil }
      guard actions.count <= 1 else {
        throw UsageError(message: "use one of \(actions.joined(separator: ", ")) at a time", command: name)
      }
      guard let option = actions.first, let value = options.values[option] else {
        return .mcp(project: options.values["--project"], action: .serve)
      }
      if value.trimmingCharacters(in: .whitespaces).isEmpty {
        throw UsageError(message: "\(option) needs \(option == "--pair" ? "the code the Mac shows" : "the name of a Mac")", command: name)
      }
      if option != "--remote", options.values["--project"] != nil {
        throw UsageError(message: "--project goes with --remote or alone, not with \(option)", command: name)
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
        throw UsageError(message: "there is no schedule command “\(options.paths[0])”", command: name)
      }
      guard options.paths.count == 2 else {
        throw UsageError(message: "name one project folder", command: name)
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
      guard options.paths.count <= 1 else { throw UsageError(message: "name one test file or run folder", command: name) }
      var request = IssueRequest(target: options.paths.first, project: options.values["--project"])
      request.send = options.flags.contains("--send")
      request.title = options.values["--title"]
      request.newIssue = options.flags.contains("--new")
      if let number = options.values["--comment-on"] {
        guard let issue = UInt64(number.drop { $0 == "#" }) else {
          throw UsageError(message: "--comment-on needs the number of an issue, such as 12, not “\(number)”", command: name)
        }
        request.commentOn = issue
      }
      if request.commentOn != nil, request.newIssue {
        throw UsageError(message: "use either --comment-on or --new, not both", command: name)
      }
      if !request.send, request.commentOn != nil || request.newIssue {
        throw UsageError(message: "--comment-on and --new say where --send files the report; add --send", command: name)
      }
      return .issue(request)
    default:
      let what = name.hasPrefix("-") ? "option" : "command"
      throw UsageError(message: "there is no \(what) “\(name)”")
    }
  }
}

extension Command {
  /// `blocks list|install <source>|update [id]|remove <id>`.
  private static func blocksRequest(_ options: Options) throws(UsageError) -> BlocksRequest {
    let name = "blocks"
    let (action, arguments) = (options.paths[0], Array(options.paths.dropFirst()))
    func argument(_ what: String) throws(UsageError) -> String {
      guard arguments.count == 1 else { throw UsageError(message: "name one \(what)", command: name) }
      return arguments[0]
    }
    let blocks: BlocksRequest.Action
    switch action {
    case "list":
      guard arguments.isEmpty else { throw UsageError(message: "blocks list takes no arguments", command: name) }
      blocks = .list
    case "install": blocks = .install(source: try argument("block folder, .zip file or https address"))
    case "remove": blocks = .remove(id: try argument("block id, such as com.acme.sign-in"))
    case "update":
      guard arguments.count <= 1 else { throw UsageError(message: "name one block id, or none for all", command: name) }
      blocks = .update(id: arguments.first)
    default:
      throw UsageError(message: "there is no blocks command “\(action)”", command: name)
    }
    var request = BlocksRequest(action: blocks, project: options.values["--project"])
    request.yes = options.flags.contains("--yes")
    request.force = options.flags.contains("--force")
    if request.yes, !["install", "update"].contains(action) {
      throw UsageError(message: "--yes belongs to install and update", command: name)
    }
    if request.force, action == "list" {
      throw UsageError(message: "--force belongs to install, update and remove", command: name)
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
        guard parts.count == 1 else { throw UsageError(message: "\(name) takes no value", command: command) }
        options.flags.insert(name)
        continue
      }
      if optionalNumbers.contains(name) {
        guard options.flags.insert(name).inserted else { throw UsageError(message: "\(name) is given twice", command: command) }
        if parts.count == 2 {
          options.values[name] = parts[1]
        } else if let next = rest.first, next.allSatisfy({ $0 >= "0" && $0 <= "9" }) {
          options.values[name] = rest.removeFirst()
        }
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
  static let commands = ["run", "validate", "fmt", "issue", "mcp", "schedule", "blocks"]

  static func usage(_ command: String?) -> String {
    switch command {
    case "run": "Usage: jevision run <tests>… [options]"
    case "validate": "Usage: jevision validate <tests>… [--json]"
    case "fmt": "Usage: jevision fmt <tests>… [--check]"
    case "issue": "Usage: jevision issue [<test or run>] [--send] [options]"
    case "mcp": "Usage: jevision mcp [--project <folder>]\n       jevision mcp --pair <code>\n       jevision mcp --remote <Mac> [--project <name>]\n       jevision mcp --forget <Mac>"
    case "schedule": "Usage: jevision schedule run <project>"
    case "blocks": "Usage: jevision blocks list | install <folder, .zip or https address> | update [<id>] | remove <id> [options]"
    default: "Usage: jevision <command> [options]"
    }
  }

  static func text(_ command: String?) -> String {
    switch command {
    case "run":
      """
      Runs tests on this Mac, one after another (or side by side with --parallel), and
      saves a report of every run.
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
                            in the app (Settings → AI): anthropic, openai, gemini, jev,
                            ollama, lmstudio or openai-compatible
        --model <name>      AI model for checks by meaning instead of the chosen one
        --junit <file>      Also save a JUnit XML report of all the tests, for CI
        --html <folder>     Also save the HTML report of every run in this folder
        --video <when>      Record a video of the app: off, on-failure (keep it only
                            when the test fails) or always. Default: as chosen in the
                            app (Settings → Advanced), otherwise on-failure
        --type <type>       Run only the named tests whose card says this type: smoke,
                            regression or acceptance
        --priority <level>  Run only tests with this priority: critical, high, medium
                            or low
        --status <status>   Run only tests with this status: draft, ready or outdated
        --author <name>     Run only tests written by this author (case doesn't matter)
        --flaky             Run only flaky tests: marked flaky in their card, or found
                            flaky from their runs of the last 30 days. Filters combine:
                            each test must fit all of them. A test whose card doesn't
                            say a field the filter asks for isn't run. Fails when no
                            test fits
        --include-quarantined-failures
                            Count failures of tests in quarantine (listed in
                            jevision.project.json, until the date set there, if any)
                            like any other failure. Without it they still run, but
                            don't change the exit status and are skipped in the JUnit
                            report. Once the date is past, the test counts again
        --parallel [<n>]    Run up to <n> tests at the same time (1 to 16; without a
                            number, one per core of this Mac) instead of one after
                            another. Tests that click, type, look at the screen or use
                            the clipboard still run one at a time; only tests that
                            leave the screen alone (commands, requests, files,
                            databases, scripts, dates) run beside them. A test starts
                            after the tests it runs after. Each test's lines are
                            printed together when it ends, and reports keep the order
                            of the tests. A folder named with digits only needs ./ in
                            front: --parallel 4 ./2024
        --no-notify         Don't send the message about the runs that
                            jevision.notifications.json in the project folder asks for
                            (Slack, Telegram, email): one message for one test, one
                            summary for several. A stopped run sends nothing. The
                            webhook, bot token and mail password come from the
                            Keychain, or on CI from JEVISION_SLACK_WEBHOOK,
                            JEVISION_TELEGRAM_TOKEN and JEVISION_SMTP_PASSWORD
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

      Exit status: 0 — all good, 1 — problems found, 2 — wrong arguments, a folder with
      no tests, or a file that could not be read.

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
      a test, 2 — wrong arguments, a folder with no tests, or a file that could not be
      read or written.

      """
    case "issue":
      """
      Writes a bug report about a failed run: the steps, what was expected and what
      happened, the cause, screenshots of the failed step and this Mac's versions. It is
      printed for you to read; with --send it is filed as an issue of the project's GitHub
      repository. Secret values are replaced with ••••, and areas the project always hides
      are painted over in the screenshots, which are saved in the repository's
      jevision-attachments branch.

      \(usage(command))

      Arguments:
        <test or run>        A test file (.flow.json), to report its newest failed run, or
                             the folder of a run in .jevision/runs. Default: the newest
                             failed run of the project

      Options:
        --project <folder>   Project folder. Default: the nearest folder at or above the
                             test (or the current folder) that has a .jevision folder
        --send               File the report on GitHub and print the address of the issue
        --title <text>       Title of the issue instead of the drafted one
        --comment-on <n>     Add the report to issue n as a comment
        --new                Open a new issue even when one about the same failure is open
        -h, --help           Show this help

      Sending uses the GitHub account JEVision is signed in to (Settings → Git), or the
      token in GH_TOKEN or GITHUB_TOKEN when set. When an issue about the same failure is
      already open, nothing is sent until you choose --comment-on or --new.

      Exit status: 0 — printed or sent, 1 — not sent because an issue about the failure is
      open, 2 — wrong arguments, no failed run, or GitHub didn't take the report.

      """
    case "mcp":
      """
      Serves the tests of a project to AI assistants on this Mac, such as Claude, Cursor or
      Copilot, over the Model Context Protocol. The assistant starts this command itself and
      talks to it over standard input and output; add it to the assistant's MCP servers.

      With --remote, the assistant uses the tools of another Mac instead: its projects, its
      screen, its mouse and keyboard. Pair this computer with that Mac once: in JEVision on
      it, open Settings → iPhone & Remote → Add an assistant and run the command it shows, which is
      jevision mcp --pair with a one-time code. The traffic is end-to-end encrypted and
      opens no ports; the key of this computer stays in its Keychain.

      \(usage(command))

      Options:
        --project <folder>  Project folder whose tests the assistant may see. Default: the
                            nearest folder at or above the current one that has a .jevision
                            folder, otherwise the current folder. With --remote, the name
                            of the project the Mac shares; without it, the only one it shares
        --pair <code>       Pair this computer with the Mac showing the code
        --remote <Mac>      Serve the tools of that paired Mac, by its name
        --forget <Mac>      Forget a paired Mac on this computer. The Mac still lists it until
                            the person there disconnects it in Settings → iPhone & Remote
        -h, --help          Show this help

      Assistants can list, read, write and check tests, read reports and see the test
      format. Running tests and taking screenshots work only after you allow them in
      JEVision → Settings → AI → Assistants on the Mac that does them; an assistant on
      another Mac needs the Assistant switch on in Settings → iPhone & Remote too. Assistants never
      get the values of secrets or API keys, and can't touch files outside the project folder.

      """
    case "blocks":
      """
      Lists, installs, updates and removes the blocks others shared: a block is a set of
      steps in a folder (ID.jevblock) or a .zip file, which tests run as one step. Blocks
      live in the project, in blocks/, and the address each came from is kept in
      jevision.blocks.json so it can be updated.

      \(usage(command))

      Commands:
        list                      Show the installed blocks
        install <source>          Show what a block does; install it with --yes. The source
                                  is a block folder, a .zip file, or an https address of a
                                  .zip file, a GitHub repository or a GitHub release
        update [<id>]             Fetch the newest version of a block installed from an
                                  address, or of all of them; install it with --yes
        remove <id>               Remove a block

      Options:
        --project <folder>        Project folder. Default: the nearest folder at or above
                                  the current one that has a .jevision folder, otherwise
                                  the current folder
        --yes                     Install or update after showing what the block does
        --force                   Remove a block that tests use, install the version that
                                  is installed again, or go back to an older version
        -h, --help                Show this help

      Installing shows the steps in plain words and, apart, the steps that run a script,
      send network requests, use a database, open apps, type on the keyboard or use the
      clipboard. Nothing is installed without --yes, and nothing in a block runs while it
      is installed. Only https addresses are downloaded.

      Exit status: 0 — done, 1 — nothing was installed or removed because of what the
      block or the project says, or because --yes or --force is missing, 2 — the project,
      block or address couldn’t be read.

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
        issue      Report a failed run as a GitHub issue
        mcp        Let AI assistants read, edit and run tests (MCP server)
        schedule   Run the schedules of a project that are due now
        blocks     Install, update and remove blocks others shared

      Options:
        -h, --help     Show this help
        -V, --version  Show the version

      Run “jevision <command> --help” for the options of a command.

      """
    }
  }
}
