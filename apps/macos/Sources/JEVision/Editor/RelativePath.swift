import Foundation
import JEVisionCore
import Synchronization

/// Paths in a test are written relative to the test file's folder, with `/`. A file can be named
/// by its label instead, `res:…`, which stays the same when the file is moved in its project.
enum RelativePath {
  /// The project folders of the tests the app opened: a label is looked up in the innermost of
  /// them that holds the test, as the core does.
  private static let projects = Mutex<Set<URL>>([])
  /// `/p/tests/refs/a.png` from `/p/tests` → `refs/a.png`; from `/p/tests/x` → `../refs/a.png`.
  static func from(_ folder: URL, to file: URL) -> String {
    let base = folder.standardizedFileURL.pathComponents
    let target = file.standardizedFileURL.pathComponents
    let shared = zip(base, target).prefix { $0 == $1 }.count
    let up = Array(repeating: "..", count: base.count - shared)
    return (up + target[shared...]).joined(separator: "/")
  }

  /// The file `path`, as a test in `folder` names it: from the folder, or by its label.
  static func resolve(_ path: String, in folder: URL) -> URL {
    folder.appending(path: Self.path(of: path, in: folder)).standardizedFileURL
  }

  /// The path from `folder` of the file `reference` names: the file a label is on, or the path
  /// itself. A label whose file is missing stays as it is.
  static func path(of reference: String, in folder: URL) -> String {
    switch resolution(reference, in: folder) {
    case (let project, .found(let path, _))?, (let project, .detached(let path, _))?:
      from(folder, to: project.appending(path: path))
    default:
      reference
    }
  }

  /// The file a preview of the picture `reference`, in a test in `folder`, shows for the dark or
  /// light appearance, the other one standing in for a missing one. A label gives the file where
  /// it is now, so a preview that reloads when this changes follows a file that was moved.
  static func previewFile(of reference: String, in folder: URL, dark: Bool) -> URL? {
    AppearanceImage.url(reference, in: folder, dark: dark, variants: true)
  }

  /// Where the file the label `reference` names is now, looked up in the project of `folder`;
  /// `nil` for a path.
  static func resolution(_ reference: String, in folder: URL) -> (project: URL, status: Resolution)? {
    guard reference.hasPrefix("res:") else { return nil }
    let project = project(of: folder)
    let status = resolveResource(project: project.path(percentEncoded: false), reference: reference)
    return status == .notAResource ? nil : (project, status)
  }

  /// Makes `project` the project of the tests in it. Its labels are read the first time, so a
  /// view that knows no project, such as a block's summary, can name their files.
  static func open(project: URL) {
    guard projects.withLock({ $0.insert(project.standardizedFileURL).inserted }) else { return }
    refreshResources(project: project.path(percentEncoded: false))
  }

  /// The project `folder` is in: the innermost one the app opened, else the nearest folder with
  /// the project's settings, a `.jevision` or a `.git` folder, else `folder` itself.
  static func project(of folder: URL) -> URL {
    let folder = folder.standardizedFileURL
    let opened = projects.withLock { $0 }.filter { $0.pathComponents == folder.pathComponents || isInside(folder, folder: $0) }
    if let innermost = opened.max(by: { $0.pathComponents.count < $1.pathComponents.count }) { return innermost }
    let markers = [projectSettingsFileName(), ".jevision", ".git"]
    let found = sequence(first: folder) { $0.pathComponents.count > 1 ? $0.deletingLastPathComponent() : nil }
      .first { parent in markers.contains { FileManager.default.fileExists(atPath: parent.appending(path: $0).path) } }
    return found ?? folder
  }

  static func isInside(_ file: URL, folder: URL) -> Bool {
    let root = folder.standardizedFileURL.pathComponents
    let parts = file.standardizedFileURL.pathComponents
    return parts.count > root.count && Array(parts.prefix(root.count)) == root
  }

  /// A file name for a test called `name`: characters a file name can't hold become dashes.
  static func testFileName(_ name: String) -> String {
    let clean = name.trimmingCharacters(in: .whitespaces)
      .replacing(/[\/:\\]/, with: "-")
      .trimmingCharacters(in: CharacterSet(charactersIn: "."))
    return (clean.isEmpty ? String(localized: "Untitled") : clean) + ProjectScanner.testSuffix
  }

  /// `name`, or `name 2`, `name 3`… — the first that `exists` says is free.
  static func unique(_ name: String, exists: (String) -> Bool) -> String {
    guard exists(name) else { return name }
    return (2...).lazy.map { "\(name) \($0)" }.first { !exists($0) }!
  }
}

/// Whether the files chosen in a project are named by their label, which the first test that
/// needs one asks about: a project can keep plain paths, so older JEVision versions open it.
enum FileLabels {
  private static func key(_ project: URL) -> String {
    "keepsPlainPaths:" + NSString(string: project.path(percentEncoded: false)).standardizingPath
  }

  static func keepsPlainPaths(in project: URL) -> Bool {
    UserDefaults.standard.bool(forKey: key(project))
  }

  static func setKeepsPlainPaths(_ keeps: Bool, in project: URL) {
    if keeps { UserDefaults.standard.set(true, forKey: key(project)) } else { UserDefaults.standard.removeObject(forKey: key(project)) }
  }

  /// What a test in `folder` writes for `file`, chosen in Finder: its label, made when it has
  /// none, or its path in a project that keeps plain paths. A file that can't have a label, such
  /// as one outside the project, is named by its path, with the reason.
  static func reference(to file: URL, from folder: URL) -> (reference: String, problem: String?) {
    let project = RelativePath.project(of: folder)
    let path = RelativePath.from(folder, to: file)
    guard !keepsPlainPaths(in: project) else { return (path, nil) }
    do {
      return (try labelResource(project: project.path(percentEncoded: false), file: file.path(percentEncoded: false)), nil)
    } catch {
      return (path, message(error))
    }
  }

  /// The name of the file `path` names, for a view that knows no project: its last part, or for
  /// a label the name of its file, `nil` when no open project has the label.
  static func fileName(_ path: String) -> String? {
    path.hasPrefix("res:") ? resourceName(reference: path) : (path as NSString).lastPathComponent
  }

  /// What went wrong with a label, in the words of the problem the core names.
  static func message(_ error: Error) -> String {
    if case ResourceError.Failed(let issue) = error { IssueText.message(issue) } else { error.localizedDescription }
  }

  /// The sentence of the problem `code` about a label, such as `resource.missing`.
  static func sentence(_ code: String, _ params: KeyValuePairs<String, String>) -> String {
    IssueText.message(
      Issue(code: code, severity: .warning, params: params.map { IssueParam(name: $0.key, value: $0.value) }))
  }
}
