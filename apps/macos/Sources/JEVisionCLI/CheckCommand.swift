import Foundation
import JEVisionCore

/// `jevision check`: tells whether a project carries every file its tests read, so it runs the
/// same once copied, cloned or on CI.
struct CheckCommand {
  /// The project folder; `nil` for the project the current folder is in.
  let project: String?
  let json: Bool
  var output = Console.standardOutput

  func execute() -> ExitStatus {
    let given = project ?? FileManager.default.currentDirectoryPath
    guard let real = RunCommand.realPath(given), isFolder(real) else {
      return fail(UsageError(message: words("cli.error.noFolder", given), code: "noFolder"))
    }
    let root = project == nil ? enclosingProject(real) ?? real : real
    let tests = checkProject(project: root)
    // A project without tests would pass without checking anything, as one opened by mistake would.
    guard !tests.isEmpty else {
      return fail(UsageError(message: words("cli.error.noTests", root), code: "noTests"))
    }
    let issues = tests.flatMap(\.issues)
    if json {
      let value = JSON.result([
        ("tests", .array(tests.map { test in
          .object([
            ("file", .string(test.file)), ("valid", .bool(!test.issues.contains { $0.severity == .error })),
            ("issues", .array(test.issues.map(\.json))),
          ])
        })),
      ])
      output.line(value.pretty)
    } else {
      for test in tests where !test.issues.isEmpty {
        printIssues(test.issues, of: test.file, to: output)
      }
      let errors = issues.count { $0.severity == .error }
      output.line(
        issues.isEmpty
          ? words("cli.check.clean", tests.count)
          : words(
            "cli.check.summary", tests.count, words("cli.validate.errors", errors),
            words("cli.validate.warnings", issues.count - errors)))
    }
    return max(exitStatus(issues), issues.contains { $0.code == "check.unreadable" } ? .io : .ok)
  }

  /// Tells why the project can't be checked, also as JSON when that is asked for.
  private func fail(_ error: UsageError) -> ExitStatus {
    Console.standardError.line("jevision: " + error.message)
    if json { output.line(JSON.result([("error", error.json)]).pretty) }
    return error.status
  }
}
