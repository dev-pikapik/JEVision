import Foundation
import JEVisionCore

/// `jevision validate`: lists the errors and warnings of every test.
func validate(_ paths: [String], json: Bool, output: Console = .standardOutput) -> ExitStatus {
  var (files, problem) = collectTests(paths, reportingEmptyFolders: true)
  var status = ExitStatus.ok
  var reports: [(file: String, issues: [Issue])] = []
  for file in files {
    guard let text = read(file, problem: &problem) else { continue }
    let issues: [Issue]
    do {
      let folder = URL(filePath: file).deletingLastPathComponent().path
      let flow = try loadFlow(json: text)
      issues = validateFlowIn(flow: flow, folder: folder) + dependencyProblems(flow, file)
    } catch let FlowError.Invalid(found) {
      issues = found
    } catch {
      issues = []
    }
    status = max(status, exitStatus(issues))
    reports.append((file, issues))
  }
  if json {
    let tests = reports.map {
      JSON.object([("file", .string($0.file)), ("valid", .bool(isValid($0.issues))), ("issues", .array($0.issues.map(\.json)))])
    }
    output.line(JSON.result([("tests", .array(tests))] + (problem.map { [("error", $0.json)] } ?? [])).pretty)
  } else {
    for report in reports { printIssues(report.issues, of: report.file, to: output) }
  }
  return max(status, problem?.status ?? .ok)
}

/// Tests `flow`, saved as `file`, runs after that don't exist or that run after it in turn,
/// looked up in the project the test would run in.
private func dependencyProblems(_ flow: Flow, _ file: String) -> [Issue] {
  guard !flow.dependsOn.isEmpty, let real = RunCommand.realPath(file) else { return [] }
  let project = RunCommand.defaultProject(for: real, tests: [real])
  return dependencyIssues(flow: flow, project: project, file: String(real.dropFirst(project == "/" ? 1 : project.count + 1)))
}

/// `jevision fmt`: rewrites tests in canonical form, or with `check` only names the ones that aren't.
func format(_ paths: [String], check: Bool, output: Console = .standardOutput) -> ExitStatus {
  var (files, problem) = collectTests(paths, reportingEmptyFolders: true)
  var status = ExitStatus.ok
  var changed = 0
  for file in files {
    guard let text = read(file, problem: &problem) else { continue }
    let canonical: String
    do {
      canonical = saveFlow(flow: try loadFlow(json: text))
    } catch let FlowError.Invalid(issues) {
      printIssues(issues, of: file, to: output)
      status = max(status, exitStatus(issues))
      continue
    } catch {
      continue
    }
    guard canonical != text else { continue }
    changed += 1
    if check {
      output.line(words("cli.fmt.notFormatted", file))
      status = max(status, .problems)
    } else {
      do {
        try writeAtomically(canonical, to: file)
        output.line(words("cli.fmt.formatted", file))
      } catch {
        tell(UsageError(message: words("cli.error.cannotWrite", file, describe(error)), code: "cannotWrite", status: .io), &problem)
      }
    }
  }
  if !files.isEmpty {
    output.line(check ? words("cli.fmt.summaryCheck", files.count, changed) : words("cli.fmt.summary", files.count, changed))
  }
  return max(status, problem?.status ?? .ok)
}

private func isValid(_ issues: [Issue]) -> Bool {
  !issues.contains { $0.severity == .error }
}

/// How the issues of a file end a command: `.needsNewer` when a newer JEVision saved the file,
/// `.problems` for any other error.
func exitStatus(_ issues: [Issue]) -> ExitStatus {
  if issues.contains(where: { asksForNewerJevision(code: $0.code) }) { return .needsNewer }
  return isValid(issues) ? .ok : .problems
}

func printIssues(_ issues: [Issue], of file: String, to output: Console) {
  guard !issues.isEmpty else { return output.line(words("cli.validate.ok", file)) }
  let errors = issues.count { $0.severity == .error }
  let warnings = issues.count - errors
  output.line(words("cli.validate.counts", file, words("cli.validate.errors", errors), words("cli.validate.warnings", warnings)))
  let (error, warning) = (words("cli.validate.error"), words("cli.validate.warning"))
  let width = max(error.count, warning.count)
  for issue in issues {
    let level = (issue.severity == .error ? error : warning).padding(toLength: width, withPad: " ", startingAt: 0)
    output.line("  \(level) \(describeIssue(issue: issue))")
  }
}

extension Issue {
  var json: JSON {
    .object([
      ("code", .string(code)),
      ("severity", .string(severity == .error ? "error" : "warning")),
      ("message", .string(describeIssue(issue: self))),
      ("params", .object(params.map { ($0.name, .string($0.value)) })),
    ])
  }
}

// MARK: Files

/// Expands folders into the `*.flow.json` files inside them, sorted by path; hidden files and
/// folders are skipped. Other paths are taken as they are. With `reportingEmptyFolders`, a folder
/// without tests is an error, so the command never ends without a word. `problem` is the error
/// that ends the command with the highest exit status, if any; each one is told on standard error.
func collectTests(_ paths: [String], reportingEmptyFolders: Bool = false) -> (files: [String], problem: UsageError?) {
  var files: [String] = []
  var problem: UsageError?
  for path in paths {
    guard isFolder(path) else {
      files.append(path)
      continue
    }
    do {
      let found = files.count
      try walk(path, into: &files)
      if reportingEmptyFolders && files.count == found {
        tell(UsageError(message: "jevision: " + words("cli.error.noTests", path), code: "noTests"), &problem)
      }
    } catch {
      tell(UsageError(message: words("cli.error.cannotRead", path, describe(error)), code: "cannotRead", status: .io), &problem)
    }
  }
  return (files, problem)
}

private func walk(_ folder: String, into files: inout [String]) throws {
  let names = try FileManager.default.contentsOfDirectory(atPath: folder)
    .sorted { $0.utf8.lexicographicallyPrecedes($1.utf8) }
  for name in names where !name.hasPrefix(".") {
    let path = folder.hasSuffix("/") ? folder + name : folder + "/" + name
    if isFolder(path) {
      try walk(path, into: &files)
    } else if name.hasSuffix(".flow.json") {
      files.append(path)
    }
  }
}

/// `given` as an absolute path without `.`, `..` or a trailing slash; a relative one is in
/// `current`.
func absolutePath(_ given: String, in current: String) -> String {
  let path = (given as NSString).expandingTildeInPath
  return ((path.hasPrefix("/") ? path : current + "/" + path) as NSString).standardizingPath
}

func isFolder(_ path: String) -> Bool {
  var isDirectory: ObjCBool = false
  return FileManager.default.fileExists(atPath: path, isDirectory: &isDirectory) && isDirectory.boolValue
}

/// Whether `folder` is a project: the app made its `.jevision` folder there, or it has the
/// environments file.
func isProjectFolder(_ folder: String) -> Bool {
  isFolder((folder as NSString).appendingPathComponent(".jevision"))
    || FileManager.default.fileExists(atPath: (folder as NSString).appendingPathComponent(environmentsFileName()))
}

/// The nearest project at or above `folder`.
func enclosingProject(_ folder: String) -> String? {
  var folder = folder
  while !isProjectFolder(folder) {
    guard folder != "/" else { return nil }
    folder = (folder as NSString).deletingLastPathComponent
  }
  return folder
}

private func read(_ file: String, problem: inout UsageError?) -> String? {
  do {
    let data = try Data(contentsOf: URL(filePath: file))
    guard let text = String(data: data, encoding: .utf8) else {
      throw CocoaError(.fileReadInapplicableStringEncoding)
    }
    return text
  } catch {
    let missing = (error as? CocoaError)?.code == .fileReadNoSuchFile
    tell(
      UsageError(message: words("cli.error.cannotRead", file, describe(error)), code: "cannotRead", status: missing ? .usage : .io),
      &problem)
    return nil
  }
}

/// Writes through a temporary file in the same folder, so a crash never leaves half a test.
private func writeAtomically(_ text: String, to path: String) throws {
  let folder = (path as NSString).deletingLastPathComponent
  let temporary = (folder as NSString).appendingPathComponent(".\((path as NSString).lastPathComponent).tmp")
  try Data(text.utf8).write(to: URL(filePath: temporary))
  do {
    if let mode = try? FileManager.default.attributesOfItem(atPath: path)[.posixPermissions] {
      try FileManager.default.setAttributes([.posixPermissions: mode], ofItemAtPath: temporary)
    }
    guard rename(temporary, path) == 0 else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
  } catch {
    try? FileManager.default.removeItem(atPath: temporary)
    throw error
  }
}

/// Says `error` on standard error and keeps it as `problem` unless that one ends the command
/// with a higher exit status already.
private func tell(_ error: UsageError, _ problem: inout UsageError?) {
  Console.standardError.line(error.message)
  if error.status > problem?.status ?? .ok { problem = error }
}

/// The system's own words for a file error, such as “No such file or directory”.
func describe(_ error: Error) -> String {
  let error = error as NSError
  let posix = error.domain == NSPOSIXErrorDomain ? error : error.userInfo[NSUnderlyingErrorKey] as? NSError
  if let posix, posix.domain == NSPOSIXErrorDomain { return String(cString: strerror(Int32(posix.code))) }
  if error.code == CocoaError.fileReadInapplicableStringEncoding.rawValue { return words("cli.error.notUTF8") }
  return error.localizedDescription
}
