import AppKit

/// Connects this Mac to GitHub in one step: Terminal opens and signs GitHub CLI in through the
/// browser, installing GitHub CLI with Homebrew first when it's missing. Without Homebrew, the
/// GitHub CLI page opens instead.
@MainActor
enum GitHubConnect {
  static func start() {
    let gh = tool("gh")
    let brew = tool("brew")
    guard gh != nil || brew != nil else {
      NSWorkspace.shared.open(URL(string: "https://cli.github.com")!)
      return
    }
    let login = "\"${GH:-gh}\" auth login --hostname github.com --git-protocol https --web"
    var lines = ["#!/bin/zsh", "clear"]
    if let gh {
      lines.append("GH=\(quoted(gh))")
    } else if let brew {
      lines += [
        "echo \(quoted(String(localized: "Installing GitHub CLI…")))",
        "\(quoted(brew)) install gh || exit 1",
        "GH=\(quoted((brew as NSString).deletingLastPathComponent + "/gh"))",
      ]
    }
    lines += [
      login,
      "echo",
      "echo \(quoted(String(localized: "Done. You can close this window and go back to JEVision.")))",
    ]
    let script = FileManager.default.temporaryDirectory.appending(path: "Connect GitHub.command")
    do {
      try (lines.joined(separator: "\n") + "\n").write(to: script, atomically: true, encoding: .utf8)
      try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: script.path)
    } catch {
      NSWorkspace.shared.open(URL(string: "https://cli.github.com")!)
      return
    }
    let terminal = URL(filePath: "/System/Applications/Utilities/Terminal.app")
    NSWorkspace.shared.open([script], withApplicationAt: terminal, configuration: NSWorkspace.OpenConfiguration())
  }

  /// The tool where Homebrew and the GitHub CLI installer put it.
  private static func tool(_ name: String) -> String? {
    ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"]
      .map { "\($0)/\(name)" }
      .first { FileManager.default.isExecutableFile(atPath: $0) }
  }

  private static func quoted(_ text: String) -> String {
    "'" + text.replacingOccurrences(of: "'", with: "'\\''") + "'"
  }
}
