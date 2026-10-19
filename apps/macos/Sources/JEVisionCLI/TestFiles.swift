import Foundation
import JEVisionCore

/// `jevision validate`: lists the errors and warnings of every test.
func validate(_ paths: [String], json: Bool, output: Console = .standardOutput) -> ExitStatus {
  var (files, status) = collectTests(paths)
  var reports: [(file: String, issues: [Issue])] = []
  for file in files {
    guard let text = read(file, status: &status) else { continue }
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
    if !isValid(issues) { status = max(status, .problems) }
    reports.append((file, issues))
  }
  if json {
    let value = JSON.array(
      reports.map { .object([("file", .string($0.file)), ("valid", .bool(isValid($0.issues))), ("issues", .array($0.issues.map(\.json)))]) })
    output.line(value.pretty)
  } else {
    for report in reports { printIssues(report.issues, of: report.file, to: output) }
  }
  return status
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
  var (files, status) = collectTests(paths)
  for file in files {
    guard let text = read(file, status: &status) else { continue }
    let canonical: String
    do {
      canonical = saveFlow(flow: try loadFlow(json: text))
    } catch let FlowError.Invalid(issues) {
      printIssues(issues, of: file, to: output)
      status = max(status, .problems)
      continue
    } catch {
      continue
    }
    guard canonical != text else { continue }
    if check {
      output.line("\(file): not formatted")
      status = max(status, .problems)
    } else {
      do {
        try writeAtomically(canonical, to: file)
        output.line("\(file): formatted")
      } catch {
        status = max(status, ioFailure(file, "write", error))
      }
    }
  }
  return status
}

private func isValid(_ issues: [Issue]) -> Bool {
  !issues.contains { $0.severity == .error }
}

private func printIssues(_ issues: [Issue], of file: String, to output: Console) {
  guard !issues.isEmpty else { return output.line("\(file): ok") }
  let errors = issues.count { $0.severity == .error }
  let warnings = issues.count - errors
  output.line("\(file): \(errors) error\(errors == 1 ? "" : "s"), \(warnings) warning\(warnings == 1 ? "" : "s")")
  for issue in issues {
    let level = issue.severity == .error ? "error  " : "warning"
    output.line("  \(level) \(describeIssue(issue: issue))")
  }
}

extension Issue {
  var json: JSON {
    .object([
      ("code", .string(code)),
      ("severity", .string(severity == .error ? "error" : "warning")),
      ("params", .object(params.map { ($0.name, .string($0.value)) })),
    ])
  }
}

// MARK: Files

/// Expands folders into the `*.flow.json` files inside them, sorted by path; hidden files and
/// folders are skipped. Other paths are taken as they are.
func collectTests(_ paths: [String]) -> (files: [String], status: ExitStatus) {
  var files: [String] = []
  var status = ExitStatus.ok
  for path in paths {
    guard isFolder(path) else {
      files.append(path)
      continue
    }
    do {
      try walk(path, into: &files)
    } catch {
      status = max(status, ioFailure(path, "read", error))
    }
  }
  return (files, status)
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

private func read(_ file: String, status: inout ExitStatus) -> String? {
  do {
    let data = try Data(contentsOf: URL(filePath: file))
    guard let text = String(data: data, encoding: .utf8) else {
      throw CocoaError(.fileReadInapplicableStringEncoding)
    }
    return text
  } catch {
    status = max(status, ioFailure(file, "read", error))
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

private func ioFailure(_ path: String, _ action: String, _ error: Error) -> ExitStatus {
  Console.standardError.line("\(path): cannot \(action): \(describe(error))")
  return .failure
}

/// The system's own words for a file error, such as “No such file or directory”.
func describe(_ error: Error) -> String {
  let error = error as NSError
  let posix = error.domain == NSPOSIXErrorDomain ? error : error.userInfo[NSUnderlyingErrorKey] as? NSError
  if let posix, posix.domain == NSPOSIXErrorDomain { return String(cString: strerror(Int32(posix.code))) }
  if error.code == CocoaError.fileReadInapplicableStringEncoding.rawValue { return "the file is not UTF-8 text" }
  return error.localizedDescription
}
