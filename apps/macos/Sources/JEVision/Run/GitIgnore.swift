import Foundation

/// Keeps run results out of git: the run folder holds screenshots, not something to commit.
enum GitIgnore {
  static let entry = ".jevision/"

  /// Whether `project` is inside a git repository that doesn't ignore its run folder yet.
  /// Reads the ignore files instead of asking git, which may not be installed.
  static func needsEntry(in project: URL) -> Bool {
    let project = project.standardizedFileURL
    var folder = project
    while true {
      if ignores(folder.appending(path: ".gitignore"), anchored: folder == project) { return false }
      let git = folder.appending(path: ".git")
      if FileManager.default.fileExists(atPath: git.path) {
        return !ignores(git.appending(path: "info/exclude"), anchored: folder == project)
      }
      if folder.pathComponents.count <= 1 { return false }
      folder = folder.deletingLastPathComponent()
    }
  }

  /// Adds the run folder to the project's own `.gitignore`, creating it if needed.
  static func add(to project: URL) throws {
    let file = project.appending(path: ".gitignore")
    var text = (try? String(contentsOf: file, encoding: .utf8)) ?? ""
    if !text.isEmpty, !text.hasSuffix("\n") { text += "\n" }
    try (text + entry + "\n").write(to: file, atomically: true, encoding: .utf8)
  }

  /// Whether the ignore file has a line for `.jevision` in its folder. `anchored` allows lines
  /// that only apply to that folder itself, like `/.jevision/`.
  private static func ignores(_ file: URL, anchored: Bool) -> Bool {
    guard let text = try? String(contentsOf: file, encoding: .utf8) else { return false }
    let anywhere: Set = [".jevision", ".jevision/", "**/.jevision", "**/.jevision/"]
    let here: Set = ["/.jevision", "/.jevision/", ".jevision/*", "/.jevision/*", ".jevision/**", "/.jevision/**"]
    return text.split(whereSeparator: \.isNewline).contains { line in
      let line = line.trimmingCharacters(in: .whitespaces)
      return anywhere.contains(line) || (anchored && here.contains(line))
    }
  }
}
