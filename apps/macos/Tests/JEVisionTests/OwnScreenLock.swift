import Foundation

/// A screen lock for the tests of this process alone. Every run takes the Mac's lock, the one in
/// `~/Library/Application Support/JEVision`, which the app, the command line and the assistants
/// share; a test that runs through the core with hands of its own never touches the screen, and
/// must not wait for the run of another program, or of another `swift test` on this Mac, nor make
/// it wait.
enum OwnScreenLock {
  private static let folder: String = {
    let made = FileManager.default.temporaryDirectory.appending(path: "jevision-screen-\(getpid())-\(UUID().uuidString)")
    try? FileManager.default.createDirectory(at: made, withIntermediateDirectories: true)
    // What the core reads at the start of every run, so it holds for all that start after this.
    setenv("JEVISION_SCREEN_LOCK", made.appending(path: "screen.lock").path, 1)
    atexit { try? FileManager.default.removeItem(atPath: OwnScreenLock.folder) }
    return made.path
  }()

  /// Makes the runs that start from now on in this process take the lock of its own.
  static func use() { _ = folder }
}
