import AppKit
import ApplicationServices
import Foundation
import JEVisionHands

/// Whether this Mac can run tests right now: a person is logged in, the screen is unlocked and
/// macOS lets jevision look at the screen and use the mouse and keyboard.
enum Readiness {
  /// What stands in the way, in words for the person at the terminal; `nil` when nothing does.
  static func problem() -> String? {
    guard CGSessionCopyCurrentDictionary() != nil else { return words("cli.ready.noUser") }
    if MacHands.screenIsLocked { return words("cli.ready.locked") }
    let canRecord = CGPreflightScreenCaptureAccess()
    let canControl = AXIsProcessTrusted()
    guard !canRecord || !canControl else { return nil }
    let lists = (canRecord ? [] : [words("cli.ready.screenRecording")]) + (canControl ? [] : [accessibilityList])
    let app = responsibleApp()
    let viaSSH = isSSHServer(app.path)
    let lines = [
      canRecord
        ? words("cli.ready.notAllowedControl", app.name)
        : canControl ? words("cli.ready.notAllowedRecord", app.name) : words("cli.ready.notAllowedBoth", app.name),
      viaSSH ? words("cli.ready.viaSSH") : words("cli.ready.viaApp"),
      "",
    ]
    let steps = lists.enumerated().map { index, list in "  " + words("cli.ready.step", index + 1, list, app.name) }
    let hints = [
      viaSSH ? words("cli.ready.chooseSSH", app.name, app.path) : words("cli.ready.choose", app.name, app.path),
      words("cli.ready.reopen", app.name),
    ] + (viaSSH ? [words("cli.ready.sshWarning")] : [])
    return (lines + steps + hints.map { "  " + $0 }).joined(separator: "\n")
  }

  /// The program macOS holds responsible for a command started over SSH: `sshd-session` on
  /// recent macOS, `sshd-keygen-wrapper` on older ones.
  static func isSSHServer(_ path: String) -> Bool {
    path.hasPrefix("/usr/libexec/sshd")
  }

  /// The list macOS 27 renamed from Accessibility.
  private static var accessibilityList: String {
    let renamed = ProcessInfo.processInfo.isOperatingSystemAtLeast(OperatingSystemVersion(majorVersion: 27, minorVersion: 0, patchVersion: 0))
    return renamed ? words("cli.ready.deviceControl") : words("run.permission.accessibility")
  }

  /// The app macOS asks permissions for when jevision runs: Terminal, an editor, a CI agent —
  /// or jevision itself when nothing launched it on its behalf.
  static func responsibleApp() -> (name: String, path: String) {
    var pid = getpid()
    // RTLD_DEFAULT: look the function up in every loaded library.
    if let lookup = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "responsibility_get_pid_responsible_for_pid") {
      typealias Responsible = @convention(c) (pid_t) -> pid_t
      let responsible = unsafeBitCast(lookup, to: Responsible.self)(pid)
      if responsible > 0 { pid = responsible }
    }
    var buffer = [CChar](repeating: 0, count: 4 * Int(MAXPATHLEN))
    guard proc_pidpath(pid, &buffer, UInt32(buffer.count)) > 0 else {
      return ("jevision", CommandLine.arguments[0])
    }
    let path = buffer.withUnsafeBufferPointer { String(cString: $0.baseAddress!) }
    // A helper inside an app asks on behalf of the outermost app.
    let parts = path.split(separator: "/", omittingEmptySubsequences: false)
    if pid != getpid(), let index = parts.firstIndex(where: { $0.hasSuffix(".app") }) {
      let app = parts[...index].joined(separator: "/")
      return (FileManager.default.displayName(atPath: app), app)
    }
    return ((path as NSString).lastPathComponent, path)
  }
}
