import Foundation
import JEVisionCore
import Testing

@testable import JEVisionCLI

struct BlocksCommandTests {
  private static let block = """
    {"format": 1, "name": "Sign in", "app": "com.apple.TextEdit", "start": "a", "blocks": {
      "a": {"type": "type", "text": "${input.email}", "next": "b"},
      "b": {"type": "command", "command": "say hello\\necho done"}}}
    """

  /// The process exit statuses; Swift Testing has an `ExitStatus` of its own.
  private enum Exit {
    static let ok: Int32 = 0
    static let problems: Int32 = 1
    static let failure: Int32 = 2
  }

  private final class Printed: @unchecked Sendable {
    private let lock = NSLock()
    private var parts: [String] = []
    func append(_ part: String) { lock.withLock { parts.append(part) } }
    var text: String { lock.withLock { parts.joined() } }
  }

  private func folder() throws -> String {
    let url = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: url.appending(path: "blocks"), withIntermediateDirectories: true)
    return url.path
  }

  /// A zip of the block, made in a project of its own.
  private func zip(_ version: String = "1.0.0") throws -> String {
    let mine = try folder()
    try Self.block.write(toFile: mine + "/blocks/Sign in.block.json", atomically: true, encoding: .utf8)
    let file = mine + "/sign-in.zip"
    let share = BlockShare(
      id: "com.anna.sign-in", name: "", description: "Types an email", author: "Anna", version: version,
      license: "MIT", inputs: [BlockPackageInput(name: "email", label: nil, defaultValue: nil)])
    _ = try exportBlockPackage(
      project: mine, blockFile: "blocks/Sign in.block.json", share: share, destination: file, asZip: true)
    return file
  }

  private func run(
    _ action: BlocksRequest.Action, in project: String, yes: Bool = false, force: Bool = false
  ) -> (status: Int32, out: String, err: String) {
    var request = BlocksRequest(action: action, project: project)
    request.yes = yes
    request.force = force
    var command = BlocksCommand(request: request)
    let (out, err) = (Printed(), Printed())
    command.output = Console(write: { out.append($0) })
    command.errors = Console(write: { err.append($0) })
    return (command.execute().rawValue, out.text, err.text)
  }

  @Test func parsesTheSubcommandsAndOptions() throws {
    #expect(try Command.parse(["blocks"]) == .help("blocks"))
    #expect(try Command.parse(["blocks", "--help"]) == .help("blocks"))
    #expect(try Command.parse(["help", "blocks"]) == .help("blocks"))
    #expect(try Command.parse(["blocks", "list"]) == .blocks(BlocksRequest(action: .list)))
    var install = BlocksRequest(action: .install(source: "https://github.com/anna/sign-in"), project: "p")
    install.yes = true
    #expect(try Command.parse(["blocks", "install", "https://github.com/anna/sign-in", "--yes", "--project", "p"]) == .blocks(install))
    #expect(try Command.parse(["blocks", "update"]) == .blocks(BlocksRequest(action: .update(id: nil))))
    var remove = BlocksRequest(action: .remove(id: "com.anna.sign-in"))
    remove.force = true
    #expect(try Command.parse(["blocks", "remove", "com.anna.sign-in", "--force"]) == .blocks(remove))

    #expect(throws: UsageError.self) { try Command.parse(["blocks", "install"]) }
    #expect(throws: UsageError.self) { try Command.parse(["blocks", "install", "a", "b"]) }
    #expect(throws: UsageError.self) { try Command.parse(["blocks", "remove"]) }
    #expect(throws: UsageError.self) { try Command.parse(["blocks", "frobnicate"]) }
    #expect(throws: UsageError.self) { try Command.parse(["blocks", "list", "--yes"]) }
    #expect(throws: UsageError.self) { try Command.parse(["blocks", "remove", "x", "--yes"]) }
    #expect(throws: UsageError.self) { try Command.parse(["blocks", "list", "--force"]) }
  }

  @Test func installingShowsWhatTheBlockDoesAndInstallsNothingWithoutYes() throws {
    let project = try folder()
    let shown = run(.install(source: try zip()), in: project)
    #expect(shown.status == Exit.problems)
    #expect(shown.out.contains("Sign in 1.0.0 by Anna (MIT)"))
    #expect(shown.out.contains("com.anna.sign-in"))
    #expect(shown.out.contains("What it does:"))
    #expect(shown.out.contains("  1. "))
    #expect(shown.out.contains("  2. "))
    #expect(shown.out.contains("Needs your attention:"))
    #expect(shown.out.contains("Runs a script on this Mac"))
    #expect(shown.out.contains("Types on the keyboard"))
    #expect(shown.out.contains("    step 2: say hello\n      echo done"), "A script is shown whole, line by line")
    #expect(shown.out.contains("Asks for: email"))
    #expect(shown.out.contains("Not installed: add --yes to install it."))
    #expect(listCommunityBlocks(project: project).isEmpty)
  }

  @Test func installsListsAndRemovesWithYes() throws {
    let project = try folder()
    let archive = try zip()
    let installed = run(.install(source: archive), in: project, yes: true)
    #expect(installed.status == Exit.ok)
    #expect(installed.out.contains("Installed Sign in 1.0.0 (com.anna.sign-in)."))

    let listed = run(.list, in: project)
    #expect(listed.status == Exit.ok)
    #expect(listed.out.contains("com.anna.sign-in  1.0.0  Sign in, by Anna"))

    let again = run(.install(source: archive), in: project, yes: true)
    #expect(again.status == Exit.ok)
    #expect(again.out.contains("installed already"))
    #expect(run(.install(source: archive), in: project, yes: true, force: true).out.contains("Installed Sign in"))

    let removed = run(.remove(id: "com.anna.sign-in"), in: project)
    #expect(removed.status == Exit.ok)
    #expect(removed.out.contains("Removed com.anna.sign-in."))
    #expect(run(.list, in: project).out.contains("No blocks are installed"))
  }

  @Test func updatesToANewerVersionAndRefusesToGoBackWithoutForce() throws {
    let project = try folder()
    let (old, new) = (try zip("1.0.0"), try zip("1.1.0"))
    #expect(run(.install(source: old), in: project, yes: true).status == Exit.ok)

    let shown = run(.install(source: new), in: project)
    #expect(shown.out.contains("Version 1.0.0 is installed; this is newer."))
    #expect(shown.status == Exit.problems)
    #expect(run(.install(source: new), in: project, yes: true).status == Exit.ok)
    #expect(listCommunityBlocks(project: project).map(\.manifest.version) == ["1.1.0"])

    let back = run(.install(source: old), in: project, yes: true)
    #expect(back.status == Exit.problems)
    #expect(back.err.contains("add --force to go back"))
    #expect(listCommunityBlocks(project: project).map(\.manifest.version) == ["1.1.0"])
    #expect(run(.install(source: old), in: project, yes: true, force: true).status == Exit.ok)
    #expect(listCommunityBlocks(project: project).map(\.manifest.version) == ["1.0.0"])
  }

  @Test func namesTheTestsThatUseABlockBeforeRemovingIt() throws {
    let project = try folder()
    #expect(run(.install(source: try zip()), in: project, yes: true).status == Exit.ok)
    try """
      {"format": 1, "name": "T", "app": "com.apple.TextEdit", "start": "a", "blocks": {
        "a": {"type": "runTest", "test": "blocks/com.anna.sign-in.jevblock/flow.json", "inputs": {"email": "a@b.c"}}}}
      """.write(toFile: project + "/Login.flow.json", atomically: true, encoding: .utf8)

    let refused = run(.remove(id: "com.anna.sign-in"), in: project)
    #expect(refused.status == Exit.problems)
    #expect(refused.err.contains("Login.flow.json"))
    #expect(refused.err.contains("--force"))
    #expect(listCommunityBlocks(project: project).count == 1)
    #expect(run(.remove(id: "com.anna.sign-in"), in: project, force: true).status == Exit.ok)
    #expect(listCommunityBlocks(project: project).isEmpty)
  }

  @Test func saysWhyABlockCantBeInstalled() throws {
    let project = try folder()
    let http = run(.install(source: "http://example.com/block.zip"), in: project, yes: true)
    #expect(http.status == Exit.failure)
    #expect(http.err.contains("only https addresses can be downloaded"))
    let missing = run(.install(source: project + "/nothing.zip"), in: project, yes: true)
    #expect(missing.status == Exit.failure)
    #expect(run(.remove(id: "com.anna.sign-in"), in: project).status == Exit.failure)
    #expect(run(.list, in: project + "/nowhere").status == Exit.failure)
    #expect(run(.update(id: nil), in: project).out.contains("nothing to update"))
  }
}
