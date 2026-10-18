import AppKit
import ApplicationServices
import Foundation
import JEVisionHands

/// Whether this Mac can run tests right now: a person is logged in, the screen is unlocked and
/// macOS lets jevision look at the screen and use the mouse and keyboard.
enum Readiness {
  /// What stands in the way, in words for the person at the terminal; `nil` when nothing does.
  static func problem() -> String? {
    guard CGSessionCopyCurrentDictionary() != nil else {
      return """
        jevision can’t run tests here: there is no logged-in user with a screen.
        Tests look at windows and click in them, so they run only in the session of a user
        who is logged in on this Mac: from Terminal, from a CI agent started in that session,
        or over SSH as that same user while they stay logged in. Not as a system service.
        For runs nobody starts by hand, turn on automatic login in System Settings → Users & Groups.
        """
    }
    if MacHands.screenIsLocked {
      return """
        jevision can’t run tests: the screen of this Mac is locked.
        Unlock it and run the command again. For runs nobody watches, turn off the automatic
        lock for this user in System Settings → Lock Screen.
        """
    }
    let canRecord = CGPreflightScreenCaptureAccess()
    let canControl = AXIsProcessTrusted()
    guard !canRecord || !canControl else { return nil }
    let lists = (canRecord ? [] : ["Screen & System Audio Recording"]) + (canControl ? [] : [accessibilityList])
    let needs = [canRecord ? nil : "record the screen", canControl ? nil : "control the computer"].compactMap(\.self)
    let app = responsibleApp()
    let steps = lists.enumerated().map { index, list in
      "  \(index + 1). Open System Settings → Privacy & Security → \(list) and turn on “\(app.name)”."
    }
    let viaSSH = isSSHServer(app.path)
    let works = viaSSH
      ? "jevision runs over SSH, so macOS asks on behalf of the SSH server, and the permission goes to it."
      : "jevision works through the app it runs in, so the permission goes to that app."
    let choose = viaSSH
      ? "click + under the list, press Command-Shift-G and type \(app.path)."
      : "click + under the list and choose \(app.path)."
    let warning = viaSSH
      ? "\n  Every command started over SSH gets these permissions: do this only on a Mac or virtual machine used just for testing."
      : ""
    return """
      jevision can’t run tests yet: macOS hasn’t allowed “\(app.name)” to \(needs.joined(separator: " and ")).
      \(works)

      \(steps.joined(separator: "\n"))
        If “\(app.name)” isn’t in the list, \(choose)
        If macOS offers to quit and reopen “\(app.name)”, do it, then run the command again.\(warning)
      """
  }

  /// The program macOS holds responsible for a command started over SSH: `sshd-session` on
  /// recent macOS, `sshd-keygen-wrapper` on older ones.
  static func isSSHServer(_ path: String) -> Bool {
    path.hasPrefix("/usr/libexec/sshd")
  }

  /// The list macOS 27 renamed from Accessibility.
  private static var accessibilityList: String {
    let renamed = ProcessInfo.processInfo.isOperatingSystemAtLeast(OperatingSystemVersion(majorVersion: 27, minorVersion: 0, patchVersion: 0))
    return renamed ? "Device Control and Data Access" : "Accessibility"
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
