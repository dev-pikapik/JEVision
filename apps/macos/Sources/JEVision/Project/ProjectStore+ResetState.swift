import JEVisionCore

extension ProjectStore {
  /// Whether an “Open App” block that starts the app fresh also gives it an empty home folder of
  /// its own; saved in the project settings file. Returns the settings as saved; `settings`
  /// follows once the project folder reports the change.
  func setCleanHome(_ cleanHome: Bool) throws -> ProjectSettings? {
    guard let rootURL else { return nil }
    return try setProjectCleanHome(project: rootURL.path(percentEncoded: false), cleanHome: cleanHome)
  }
}
