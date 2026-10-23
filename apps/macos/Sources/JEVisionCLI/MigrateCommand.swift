import Foundation
import JEVisionCore

/// What `jevision migrate` was asked to do, in which project.
struct MigrateRequest: Equatable {
  enum Action: Equatable {
    /// Moves the tests to file labels; a dry run only says what would change.
    case migrate(dryRun: Bool)
    case backups
    case restore(id: String)
  }

  var action: Action
  var project = "."
  var json = false
}

/// `jevision migrate`: names the files of a project's tests by their labels, which stay the same
/// when the files are moved or renamed; lists the copies of tests kept before such changes and
/// puts one back.
struct MigrateCommand {
  let request: MigrateRequest
  var output = Console.standardOutput

  func execute() -> ExitStatus {
    guard let root = realpath(request.project, nil).map({ path in
      defer { free(path) }
      return String(cString: path)
    }), isFolder(root) else {
      Console.standardError.line("jevision: " + words("cli.error.noFolder", request.project))
      return .failure
    }
    do {
      switch request.action {
      case .migrate(let dryRun):
        return migrate(try migrateProject(project: root, dryRun: dryRun), dryRun: dryRun)
      case .backups:
        backups(listBackups(project: root))
        return .ok
      case .restore(let id):
        restored(try restoreBackup(project: root, id: id), from: id)
        return .ok
      }
    } catch ResourceError.Failed(let issue) {
      Console.standardError.line("jevision: " + issueSentence(issue: issue))
    } catch {
      Console.standardError.line("jevision: " + describe(error))
    }
    return .failure
  }

  private func migrate(_ report: MigrationReport, dryRun: Bool) -> ExitStatus {
    let status: ExitStatus = report.problems.isEmpty ? .ok : .problems
    if request.json {
      output.line(Self.json(report, dryRun: dryRun).pretty)
      return status
    }
    if report.files.isEmpty {
      output.line(words("cli.migrate.nothing"))
    } else {
      output.line(dryRun ? words("cli.migrate.wouldChange", report.files.count) : words("cli.migrate.changed", report.files.count))
      for file in report.files {
        output.line("  " + words("cli.migrate.file", file.path, file.labels.count, file.newSidecars.count))
      }
    }
    if !report.problems.isEmpty {
      output.line(words("cli.migrate.unreadable", report.problems.count))
      for problem in report.problems {
        for issue in problem.issues { output.line("  \(problem.path): \(describeIssue(issue: issue))") }
      }
    }
    if dryRun, !report.files.isEmpty {
      output.line(words("cli.migrate.dryRun"))
    } else if let backup = report.backup {
      output.line(words("cli.migrate.backup", backup, "jevision migrate --restore \(backup)"))
    }
    return status
  }

  private func backups(_ backups: [Backup]) {
    if request.json {
      output.line(JSON.object([("backups", .array(backups.map(Self.json)))]).pretty)
      return
    }
    guard !backups.isEmpty else { return output.line(words("cli.migrate.noBackups")) }
    for backup in backups {
      output.line("\(backup.id)  \(backup.created)  \(words("cli.migrate.files", backup.files.count))")
    }
  }

  private func restored(_ files: [String], from id: String) {
    if request.json {
      output.line(JSON.object([("backup", .string(id)), ("restored", .array(files.map { .string($0) }))]).pretty)
      return
    }
    output.line(words("cli.migrate.restored", files.count, id))
    for file in files { output.line("  \(file)") }
  }

  static func json(_ report: MigrationReport, dryRun: Bool) -> JSON {
    .object([
      ("dryRun", .bool(dryRun)),
      (
        "files",
        .array(
          report.files.map { file in
            .object([
              ("path", .string(file.path)),
              ("fromFormat", .int(Int64(file.fromFormat))),
              ("toFormat", .int(Int64(file.toFormat))),
              ("labels", .array(file.labels.map { .object([("path", .string($0.path)), ("reference", .string($0.reference))]) })),
              ("newLabelFiles", .array(file.newSidecars.map { .string($0) })),
            ])
          })
      ),
      (
        "problems",
        .array(report.problems.map { .object([("path", .string($0.path)), ("issues", .array($0.issues.map(\.json)))]) })
      ),
      ("backup", .string(report.backup)),
    ])
  }

  static func json(_ backup: Backup) -> JSON {
    .object([
      ("id", .string(backup.id)),
      ("created", .string(backup.created)),
      (
        "files",
        .array(
          backup.files.map {
            .object([
              ("path", .string($0.path)), ("fromFormat", .int(Int64($0.fromFormat))), ("toFormat", .int(Int64($0.toFormat))),
            ])
          })
      ),
    ])
  }
}
