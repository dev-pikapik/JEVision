import Foundation
import JEVisionCore
import JEVisionHands

/// What `jevision mcp` was asked to do.
enum MCPAction: Equatable {
  /// Serve the tests of a project on this Mac.
  case serve
  /// Pair this computer with the Mac showing this code.
  case pair(String)
  /// Serve the tools of a paired Mac, as the Mac lets this computer use them.
  case remote(String)
  /// Forget a paired Mac.
  case forget(String)
}

/// `jevision mcp`: lets AI assistants on this Mac, or on a Mac they are paired with, read, edit
/// and run the project's tests. The server itself lives in the core; this side gives it the Mac:
/// settings, hands and secrets, or the key and the list of Macs of this computer.
struct MCPCommand {
  /// The project folder on this Mac; with `.remote`, the name of the project the other Mac shares.
  let project: String?
  var action = MCPAction.serve

  func execute() -> ExitStatus {
    switch action {
    case .serve: serveHere()
    case .pair(let link): pair(link)
    case .remote(let name): serveRemote(name)
    case .forget(let name): forget(name)
    }
  }

  private func serveHere() -> ExitStatus {
    guard let folder = project ?? Self.defaultProject() else {
      Console.standardError.line(
        "jevision: name the project folder with --project: the assistant started jevision in /, which is no project.")
      return .failure
    }
    do {
      try serveAssistants(
        project: folder, host: MacAssistantHost(readiness: { Readiness.problem() }))
      return .ok
    } catch AssistantServerError.NoProject(let path) {
      Console.standardError.line("jevision: there is no folder “\(path)”")
    } catch {
      Console.standardError.line("jevision: \(error.localizedDescription)")
    }
    return .failure
  }

  private func pair(_ link: String) -> ExitStatus {
    do {
      let key = try ComputerKey.existingOrNew()
      let mac = try pairRemoteAssistant(link: link, computerName: Self.computerName, key: key)
      try RemoteMacs.standard.save(mac)
      Console.standardOutput.line(
        """
        Paired with “\(mac.name)”. Add it to the MCP servers of your assistant, such as Claude, Cursor or Copilot, with:

          jevision mcp --remote \(shellWord(mac.name))

        The assistant can then use the tools of that Mac as far as the person there allows it in
        JEVision → Settings → iPhone & Remote and Settings → AI → Assistants.
        """)
      return .ok
    } catch {
      Console.standardError.line("jevision: \(error.localizedDescription)")
      return .failure
    }
  }

  private func serveRemote(_ name: String) -> ExitStatus {
    do {
      guard let mac = try pairedMac(name) else { return .failure }
      guard let key = ComputerKey.existing() else {
        Console.standardError.line(
          "jevision: the key of this computer isn’t in the Keychain anymore. Pair it again with `jevision mcp --pair`.")
        return .failure
      }
      try serveRemoteAssistant(mac: mac, key: key, project: project)
      return .ok
    } catch {
      Console.standardError.line("jevision: \(error.localizedDescription)")
      return .failure
    }
  }

  private func forget(_ name: String) -> ExitStatus {
    do {
      guard let mac = try pairedMac(name) else { return .failure }
      try RemoteMacs.standard.remove(mac)
      if try RemoteMacs.standard.all().isEmpty { try ComputerKey.remove() }
      Console.standardOutput.line(
        "Forgot “\(mac.name)”. The Mac still lists this computer until the person there disconnects it in JEVision → Settings → iPhone & Remote.")
      return .ok
    } catch {
      Console.standardError.line("jevision: \(error.localizedDescription)")
      return .failure
    }
  }

  /// The paired Mac that `name` means; says why on standard error and returns nil when it
  /// isn't exactly one.
  private func pairedMac(_ name: String) throws -> HostRecord? {
    let macs = try RemoteMacs.standard.matching(name)
    if macs.count == 1 { return macs[0] }
    Console.standardError.line(
      macs.isEmpty
        ? "jevision: this computer isn’t paired with a Mac called “\(name)”. Pair it in JEVision on that Mac: Settings → iPhone & Remote → Add an assistant."
        : "jevision: several paired Macs are called “\(name)”. Name one by the start of its id: \(macs.map { String($0.hostId.prefix(8)) }.joined(separator: ", ")).")
    return nil
  }

  /// This computer's name as the Macs it pairs with list it.
  static var computerName: String {
    Host.current().localizedName ?? ProcessInfo.processInfo.hostName
  }

  /// The nearest folder at or above the current one that has a `.jevision` folder, as the app
  /// makes one in the project it opens, or the environments file; otherwise the current folder, unless it is `/`.
  static func defaultProject(from current: String = FileManager.default.currentDirectoryPath) -> String? {
    var folder = current
    while folder != "/" {
      if isProjectFolder(folder) { return folder }
      folder = (folder as NSString).deletingLastPathComponent
    }
    return current == "/" ? nil : current
  }
}
