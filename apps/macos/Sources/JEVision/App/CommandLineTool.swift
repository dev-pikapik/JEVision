import AppKit
import JEVisionCore
import SwiftUI

/// The `jevision` command inside this app, and the link that puts it on the PATH.
struct CommandLineTool {
  static let name = "jevision"
  /// Needs a password; on the PATH of every Terminal.
  static let systemFolder = URL(filePath: "/usr/local/bin")
  /// Needs no password; not on the PATH by default.
  static var userFolder: URL {
    FileManager.default.homeDirectoryForCurrentUser.appending(path: ".local/bin")
  }

  /// The tool inside the app.
  let tool: URL

  static let bundled = CommandLineTool(tool: Bundle.main.bundleURL.appending(path: "Contents/Helpers/\(name)"))

  enum State: Equatable {
    /// The link opens this app's tool.
    case installed(link: URL)
    /// `jevision` there is something else, such as another copy of JEVision; its version if it says.
    case other(link: URL, version: String?)
    case notInstalled
  }

  /// Why the tool can't be installed from this copy of the app, if it can't.
  var unavailableReason: LocalizedStringKey? {
    if tool.path.contains("/AppTranslocation/") || tool.path.hasPrefix("/Volumes/") {
      return "Move JEVision to the Applications folder and open it from there first: the command must point to where the app stays."
    }
    guard FileManager.default.isExecutableFile(atPath: tool.path) else {
      return "This copy of JEVision doesn’t include the command line tool."
    }
    return nil
  }

  /// The first of `folders` that has a `jevision`, and what it is.
  func state(in folders: [URL] = [Self.systemFolder, Self.userFolder]) -> State {
    for folder in folders {
      let link = folder.appending(path: Self.name)
      guard FileManager.default.fileExists(atPath: link.path) else { continue }
      if link.resolvingSymlinksInPath().path == tool.resolvingSymlinksInPath().path { return .installed(link: link) }
      return .other(link: link, version: Self.version(of: link))
    }
    return .notInstalled
  }

  /// Links the tool into `folder` without asking for a password; replaces what was there.
  func install(in folder: URL) throws {
    let link = folder.appending(path: Self.name)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    // Doesn't follow a link, so a link to a removed copy of the app is replaced too.
    if (try? FileManager.default.attributesOfItem(atPath: link.path)) != nil {
      try FileManager.default.removeItem(at: link)
    }
    try FileManager.default.createSymbolicLink(at: link, withDestinationURL: tool)
  }

  /// Links the tool into /usr/local/bin; macOS asks for an administrator's password.
  @MainActor
  func installForEveryone() throws(InstallError) {
    let command = "mkdir -p \(Self.quoted(Self.systemFolder.path)) && ln -sfn \(Self.quoted(tool.path)) \(Self.quoted(Self.systemFolder.appending(path: Self.name).path))"
    let prompt = String(localized: "JEVision wants to add the jevision command to /usr/local/bin.")
    let source = "do shell script \(Self.appleScriptString(command)) with prompt \(Self.appleScriptString(prompt)) with administrator privileges"
    var error: NSDictionary?
    guard let script = NSAppleScript(source: source) else { throw .failed(String(localized: "The installer script couldn’t be prepared.")) }
    script.executeAndReturnError(&error)
    guard let error else { return }
    // -128: the person clicked Cancel.
    if error[NSAppleScript.errorNumber] as? Int == -128 { throw .cancelled }
    throw .failed(error[NSAppleScript.errorMessage] as? String ?? String(localized: "Unknown error."))
  }

  enum InstallError: Error, Equatable {
    case cancelled
    case failed(String)
  }

  /// What to type in Terminal to install for everyone by hand.
  var manualCommand: String {
    "sudo mkdir -p \(Self.systemFolder.path) && sudo ln -sfn \(Self.quoted(tool.path)) \(Self.systemFolder.appending(path: Self.name).path)"
  }

  /// Adds ~/.local/bin to the PATH of new zsh windows.
  static let pathCommand = #"echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zprofile"#

  /// What `jevision --version` prints at `link`, such as `0.1.0`.
  private static func version(of link: URL) -> String? {
    let process = Process()
    let output = Pipe()
    process.executableURL = link
    process.arguments = ["--version"]
    process.standardOutput = output
    process.standardError = FileHandle.nullDevice
    guard (try? process.run()) != nil else { return nil }
    let data = output.fileHandleForReading.readDataToEndOfFile()
    process.waitUntilExit()
    let words = String(decoding: data, as: UTF8.self).split(whereSeparator: \.isWhitespace)
    return process.terminationStatus == 0 && words.count == 2 && words[0] == "jevision" ? String(words[1]) : nil
  }

  /// A shell word that stays one word whatever it holds.
  static func quoted(_ text: String) -> String {
    "'" + text.replacingOccurrences(of: "'", with: #"'\''"#) + "'"
  }

  private static func appleScriptString(_ text: String) -> String {
    "\"" + text.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"") + "\""
  }
}

/// Puts the `jevision` command on the PATH, from the app menu.
struct CommandLineToolView: View {
  private let tool = CommandLineTool.bundled
  @State private var state = CommandLineTool.State.notInstalled
  /// The password prompt was cancelled or failed: offer the ways that need none.
  @State private var showsAlternatives = false
  @State private var failure: String?

  var body: some View {
    Form {
      Section {
        PageHeader(
          symbol: "apple.terminal.fill", color: .gray, title: Text("Command Line Tool"),
          message: Text("Run and check tests from Terminal, scripts and CI with the jevision command."))
      }
      Section {
        LabeledContent("Status") { status }
        if let reason = tool.unavailableReason {
          Text(reason)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        } else {
          LabeledContent {
            Button(isInstalled ? "Reinstall…" : "Install…", action: installForEveryone)
              .keyboardShortcut(isInstalled ? nil : .defaultAction)
          } label: {
            Text("Add jevision to /usr/local/bin")
            Text("macOS asks for an administrator’s password.")
          }
        }
        if let failure {
          Label(failure, systemImage: "exclamationmark.triangle.fill")
            .foregroundStyle(.red)
            .fixedSize(horizontal: false, vertical: true)
        }
      } footer: {
        if isInstalled {
          Text("Open a new Terminal window and type “jevision --help” to see what it can do.")
            .font(.callout)
            .foregroundStyle(.secondary)
        }
      }
      if showsAlternatives, tool.unavailableReason == nil {
        Section("Without an administrator’s password") {
          LabeledContent {
            Button("Install Just for Me", action: installForMe)
          } label: {
            Text("Add jevision to ~/.local/bin")
            Text("If Terminal then says “command not found”, add that folder to the PATH:")
          }
          CommandText(CommandLineTool.pathCommand)
        }
        Section("Or install it yourself") {
          Text("Run this command in Terminal and enter your password:")
          CommandText(tool.manualCommand)
        }
      }
    }
    .formStyle(.grouped)
    .frame(width: 560)
    .fixedSize(horizontal: false, vertical: true)
    .onAppear(perform: refresh)
  }

  private var isInstalled: Bool {
    if case .installed = state { true } else { false }
  }

  @ViewBuilder
  private var status: some View {
    switch state {
    case .installed(let link):
      Label("Installed in \(link.deletingLastPathComponent().path), version \(jevisionVersion())", systemImage: "checkmark.circle.fill")
        .foregroundStyle(.green)
    case .other(let link, let version?):
      Label("\(link.path) is version \(version) from another copy of JEVision", systemImage: "exclamationmark.circle.fill")
        .foregroundStyle(.orange)
    case .other(let link, nil):
      Label("\(link.path) is another program", systemImage: "exclamationmark.circle.fill")
        .foregroundStyle(.orange)
    case .notInstalled:
      Text("Not installed")
        .foregroundStyle(.secondary)
    }
  }

  private func installForEveryone() {
    failure = nil
    do {
      try tool.installForEveryone()
    } catch {
      if case .failed(let message) = error { failure = message }
      showsAlternatives = true
    }
    refresh()
  }

  private func installForMe() {
    failure = nil
    do {
      try tool.install(in: CommandLineTool.userFolder)
    } catch {
      failure = error.localizedDescription
    }
    refresh()
  }

  private func refresh() {
    state = tool.state()
  }
}

/// A command to copy into Terminal.
struct CommandText: View {
  let command: String

  init(_ command: String) {
    self.command = command
  }

  var body: some View {
    HStack(alignment: .firstTextBaseline) {
      Text(verbatim: command)
        .font(.callout.monospaced())
        .textSelection(.enabled)
        .fixedSize(horizontal: false, vertical: true)
        .frame(maxWidth: .infinity, alignment: .leading)
      Button("Copy", systemImage: "doc.on.doc") {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(command, forType: .string)
      }
      .labelStyle(.iconOnly)
      .buttonStyle(.borderless)
      .help("Copy")
    }
  }
}
