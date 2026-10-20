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

  /// Writes the package `id` into the folder `shelf`, needing `needs` (id, versions).
  private func package(_ id: String, _ version: String = "1.0.0", in shelf: String, needs: [(String, String)] = []) throws {
    let dir = URL(fileURLWithPath: shelf).appending(path: "\(id).jevblock")
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let dependencies = needs.map { #"{"id": "\#($0.0)", "version": "\#($0.1)"}"# }.joined(separator: ", ")
    try """
      {"format": 1, "id": "\(id)", "name": "\(id)", "version": "\(version)", "author": "Acme", "license": "MIT",
       "dependencies": [\(dependencies)]}
      """.write(to: dir.appending(path: "block.json"), atomically: true, encoding: .utf8)
    try """
      {"format": 1, "name": "\(id)", "app": "com.apple.TextEdit", "start": "a", "blocks": {
        "a": {"type": "pressKeys", "keys": "cmd+s"}}}
      """.write(to: dir.appending(path: "flow.json"), atomically: true, encoding: .utf8)
  }

  private func installed(_ project: String) -> Set<String> {
    Set(listCommunityBlocks(project: project).map(\.manifest.id))
  }

  private func run(
    _ action: BlocksRequest.Action, in project: String, yes: Bool = false, force: Bool = false, only: [String] = []
  ) -> (status: Int32, out: String, err: String) {
    var request = BlocksRequest(action: action, project: project)
    request.yes = yes
    request.force = force
    request.only = only
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

    var restore = BlocksRequest(action: .restore)
    (restore.yes, restore.force, restore.only) = (true, true, ["com.a.b", "com.c.d"])
    #expect(try Command.parse(["blocks", "restore", "--yes", "--force", "--only", "com.a.b, com.c.d"]) == .blocks(restore))
    var only = BlocksRequest(action: .install(source: "shelf"))
    only.only = ["com.a.b"]
    #expect(try Command.parse(["blocks", "install", "shelf", "--only=com.a.b"]) == .blocks(only))
    #expect(throws: UsageError.self) { try Command.parse(["blocks", "restore", "x"]) }
    #expect(throws: UsageError.self) { try Command.parse(["blocks", "remove", "x", "--only", "y"]) }
    #expect(throws: UsageError.self) { try Command.parse(["blocks", "install", "x", "--only", ","]) }
  }

  @Test func restoresWhatJevisionBlocksJsonNamesOnlyWithYes() throws {
    let (shelf, project) = (try folder(), try folder())
    try package("com.acme.a", in: shelf)
    try package("com.acme.b", "2.0.0", in: shelf)
    try """
      {"format": 1, "sources": {"com.acme.a": "\(shelf)", "com.acme.b": "\(shelf)"},
       "versions": {"com.acme.a": "1.0.0", "com.acme.b": "1.5.0"}}
      """.write(toFile: project + "/jevision.blocks.json", atomically: true, encoding: .utf8)

    let plan = run(.restore, in: project)
    #expect(plan.status == Exit.problems)
    #expect(plan.out.contains("2 blocks of jevision.blocks.json aren't installed."))
    #expect(plan.out.contains("com.acme.a 1.0.0 by Acme (MIT)"))
    #expect(plan.out.contains("Not installed: add --yes to install it."))
    #expect(plan.err.contains("keeps com.acme.b 1.5.0"))
    #expect(plan.err.contains("now has 2.0.0"))
    #expect(installed(project).isEmpty, "Nothing is installed without --yes")

    let yes = run(.restore, in: project, yes: true)
    #expect(yes.status == Exit.problems, "The block whose address changed is skipped")
    #expect(yes.out.contains("Installed com.acme.a 1.0.0 (com.acme.a)."))
    #expect(installed(project) == ["com.acme.a"])

    let forced = run(.restore, in: project, yes: true, force: true)
    #expect(forced.status == Exit.ok)
    #expect(installed(project) == ["com.acme.a", "com.acme.b"])
    let done = run(.restore, in: project)
    #expect(done.status == Exit.ok)
    #expect(done.out.contains("Everything in jevision.blocks.json is installed already."))
  }

  @Test func installsASetOrOnlyTheBlocksNamedWithWhatTheyNeed() throws {
    let (shelf, project) = (try folder(), try folder())
    try package("com.acme.base", in: shelf)
    try package("com.acme.app", in: shelf, needs: [("com.acme.base", "^1.0.0")])
    try package("com.acme.extra", in: shelf)
    try package("com.acme.lonely", in: shelf, needs: [("com.acme.nowhere", "*")])

    let unknown = run(.install(source: shelf), in: project, yes: true, only: ["com.acme.nope"])
    #expect(unknown.status == Exit.problems)
    #expect(unknown.err.contains("no block com.acme.nope"))

    let shown = run(.install(source: shelf), in: project, only: ["com.acme.app"])
    #expect(shown.out.contains("4 blocks in \(shelf)"))
    #expect(shown.out.contains("  Needs: com.acme.base ^1.0.0"))
    #expect(!shown.out.contains("com.acme.extra 1.0.0"), "Only the blocks named are shown")
    #expect(shown.status == Exit.problems)

    let only = run(.install(source: shelf), in: project, yes: true, only: ["com.acme.app"])
    #expect(only.status == Exit.ok)
    #expect(only.out.contains("Installed com.acme.base 1.0.0 (com.acme.base).\nInstalled com.acme.app"), "What a block needs goes in first")
    #expect(installed(project) == ["com.acme.base", "com.acme.app"])

    let all = run(.install(source: shelf), in: project, yes: true)
    #expect(all.status == Exit.problems)
    #expect(all.err.contains("com.acme.nowhere"))
    #expect(installed(project) == ["com.acme.base", "com.acme.app"], "Nothing more goes in while one can't")
  }

  @Test func namesTheBlocksThatNeedABlockBeforeRemovingIt() throws {
    let (shelf, project) = (try folder(), try folder())
    try package("com.acme.base", in: shelf)
    try package("com.acme.app", in: shelf, needs: [("com.acme.base", "^1.0.0")])
    #expect(run(.install(source: shelf), in: project, yes: true).status == Exit.ok)
    #expect(run(.list, in: project).out.contains("needs com.acme.base ^1.0.0"))

    let refused = run(.remove(id: "com.acme.base"), in: project)
    #expect(refused.status == Exit.problems)
    #expect(refused.err.contains("needed by the installed blocks com.acme.app"))
    #expect(refused.err.contains("--force"))
    #expect(installed(project) == ["com.acme.base", "com.acme.app"])
    #expect(run(.remove(id: "com.acme.base"), in: project, force: true).status == Exit.ok)
    #expect(installed(project) == ["com.acme.app"])
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
