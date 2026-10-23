import Foundation
import JEVisionCore

/// Keeps run results out of git: the run folder holds screenshots, not something to commit.
/// The rules live in the core, which also writes the `.gitignore` of a new project.
enum GitIgnore {
  struct Failure: LocalizedError {
    let errorDescription: String?
  }

  /// Whether `project` is inside a git repository that doesn't ignore its run folder yet.
  /// Reads the ignore files instead of asking git, which may not be installed.
  static func needsEntry(in project: URL) -> Bool {
    gitignoreNeedsRunFolder(project: project.standardizedFileURL.path)
  }

  /// Adds the run folder to the project's own `.gitignore`, creating it if needed.
  static func add(to project: URL) throws {
    do {
      try gitignoreAddRunFolder(project: project.standardizedFileURL.path)
    } catch ProjectFolderError.Failed(let reason) {
      throw Failure(errorDescription: reason)
    }
  }
}
