import AppKit

enum Relaunch {
  /// Starts a tiny shell that waits for this process to exit and opens the app again.
  @MainActor
  static func now() -> Bool {
    let shell = Process()
    shell.executableURL = URL(fileURLWithPath: "/bin/sh")
    shell.arguments = [
      "-c", "while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; open \"$2\"",
      "relaunch", String(ProcessInfo.processInfo.processIdentifier), Bundle.main.bundleURL.path,
    ]
    do {
      try shell.run()
    } catch {
      return false
    }
    NSApp.terminate(nil)
    return true
  }
}
