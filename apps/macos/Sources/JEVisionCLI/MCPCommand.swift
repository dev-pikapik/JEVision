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
    do { try AppPreferences.checkApp() }
    catch {
      Console.standardError.line("jevision: \(error.localizedDescription)")
      return (error as? AppPreferences.Failure)?.status == 5 ? .needsNewer : .io
    }
    return switch action {
    case .serve: serveHere()
    case .pair(let link): pair(link)
    case .remote(let name): serveRemote(name)
    case .forget(let name): forget(name)
    }
  }

  private func serveHere() -> ExitStatus {
    guard let folder = project ?? Self.defaultProject() else {
      Console.standardError.line("jevision: " + words("cli.mcp.noProject"))
      return .usage
    }
    do {
      try serveAssistants(
        project: folder, host: MacAssistantHost(readiness: { Readiness.problem() }))
      return .ok
    } catch AssistantServerError.NoProject(let path) {
      Console.standardError.line("jevision: " + words("cli.error.noFolder", path))
      return .usage
    } catch {
      Console.standardError.line("jevision: \(error.localizedDescription)")
      return .io
    }
  }

  private func pair(_ link: String) -> ExitStatus {
    do {
      let key = try ComputerKey.existingOrNew()
      let mac = try pairRemoteAssistant(link: link, computerName: Self.computerName, key: key)
      try RemoteMacs.standard.save(mac)
      Console.standardOutput.line(words("cli.mcp.paired", mac.name, "jevision mcp --remote \(shellWord(mac.name))"))
      return .ok
    } catch {
      return Self.failed(error)
    }
  }

  private func serveRemote(_ name: String) -> ExitStatus {
    do {
      guard let mac = try pairedMac(name) else { return .usage }
      guard let key = ComputerKey.existing() else {
        Console.standardError.line("jevision: " + words("cli.mcp.noKey"))
        return .notReady
      }
      try serveRemoteAssistant(mac: mac, key: key, project: project)
      return .ok
    } catch {
      return Self.failed(error)
    }
  }

  private func forget(_ name: String) -> ExitStatus {
    do {
      guard let mac = try pairedMac(name) else { return .usage }
      try RemoteMacs.standard.remove(mac)
      if try RemoteMacs.standard.all().isEmpty { try ComputerKey.remove() }
      Console.standardOutput.line(words("cli.mcp.forgot", mac.name))
      return .ok
    } catch {
      return Self.failed(error)
    }
  }

  /// Says why the other Mac can't be paired or served: it is out of reach, or the list of paired
  /// Macs or this computer's key can't be read or written.
  private static func failed(_ error: Error) -> ExitStatus {
    Console.standardError.line("jevision: \(error.localizedDescription)")
    switch error {
    case AssistantServerError.NoProject: return .usage
    case AssistantServerError.Io: return .notReady
    default: return .io
    }
  }

  /// The paired Mac that `name` means; says why on standard error and returns nil when it
  /// isn't exactly one.
  private func pairedMac(_ name: String) throws -> HostRecord? {
    let macs = try RemoteMacs.standard.matching(name)
    if macs.count == 1 { return macs[0] }
    Console.standardError.line(
      "jevision: "
        + (macs.isEmpty
          ? words("cli.mcp.notPaired", name)
          : words("cli.mcp.severalMacs", name, macs.map { String($0.hostId.prefix(8)) }.joined(separator: ", "))))
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
