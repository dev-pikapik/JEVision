import Foundation
import JEVisionCore
import JEVisionHands

/// What `jevision blocks` was asked to do.
struct BlocksRequest: Equatable {
  enum Action: Equatable {
    case list
    /// A folder, a .zip file or an https address.
    case install(source: String)
    /// The newest version of one installed block, or of every block installed from an address.
    case update(id: String?)
    case remove(id: String)
    /// The blocks jevision.blocks.json names and the project hasn't got, each from its address.
    case restore
  }

  var action: Action
  var project: String?
  /// Install or update after showing what the block does.
  var yes = false
  /// Remove a block that tests or other blocks use, install over a newer version, install the same
  /// version again, install a block that breaks an installed one, or restore a block whose address
  /// now has another version.
  var force = false
  /// Install or restore only these blocks, with the blocks they need.
  var only: [String] = []
}

/// `jevision blocks`: lists, installs, updates and removes the blocks others shared, as the app's
/// Install Block… does. Installing first shows what the block does, and installs only with `--yes`.
struct BlocksCommand {
  let request: BlocksRequest
  var output = Console.standardOutput
  var errors = Console.standardError

  func execute() -> ExitStatus {
    guard let project = request.project ?? MCPCommand.defaultProject(), isFolder(project) else {
      errors.line("jevision: there is no folder “\(request.project ?? FileManager.default.currentDirectoryPath)”")
      return .failure
    }
    do {
      switch request.action {
      case .list: return list(project)
      case .install(let source): return try install(source, in: project)
      case .update(let id): return try update(id, in: project)
      case .remove(let id): return try remove(id, in: project)
      case .restore: return try restore(in: project)
      }
    } catch BlockPackageError.Failed(let issues) {
      for issue in issues { errors.line("jevision: \(describeIssue(issue: issue))") }
    } catch {
      errors.line("jevision: \(error.localizedDescription)")
    }
    return .failure
  }

  // MARK: Commands

  private func list(_ project: String) -> ExitStatus {
    let blocks = listCommunityBlocks(project: project)
    guard !blocks.isEmpty else {
      output.line("No blocks are installed in \(project).")
      return .ok
    }
    let width = blocks.map(\.manifest.id.count).max() ?? 0
    for block in blocks {
      let manifest = block.manifest
      let id = manifest.id.padding(toLength: width, withPad: " ", startingAt: 0)
      output.line("\(id)  \(manifest.version)  \(manifest.name), by \(manifest.author)")
      let indent = String(repeating: " ", count: width + 2)
      if let source = block.source { output.line("\(indent)from \(source)") }
      if !manifest.dependencies.isEmpty { output.line("\(indent)needs \(Self.needs(manifest))") }
    }
    return .ok
  }

  /// Shows the blocks at `source`, those `--only` names when it names some, and installs them with `--yes`.
  private func install(_ source: String, in project: String) throws -> ExitStatus {
    let packages = try inspectBlockSource(project: project, source: source).packages
    let chosen = choose(packages, id: \.manifest.id, needs: { $0.manifest.dependencies.map(\.id) }, among: source)
    for preview in packages where preview.temporary && !(chosen ?? []).contains(preview) {
      discardBlockStaging(folder: preview.folder)
    }
    guard let chosen else { return .problems }
    if packages.count > 1 {
      output.line("\(packages.count) blocks in \(source)")
      output.line()
    }
    return try install(chosen, in: project)
  }

  /// Shows `previews`, with the blocks they need read from the addresses they name, and installs
  /// them all, each after the blocks it needs, when `--yes` was given and nothing speaks against it.
  private func install(_ previews: [BlockPreview], in project: String) throws -> ExitStatus {
    var fetching = false
    let fetched = BlockDependencyFetch.fetch(
      for: previews, plan: { try planBlockInstall(project: project, folders: $0) },
      inspect: { try inspectBlockSource(project: project, source: $0) },
      discard: { discardBlockStaging(folder: $0) },
      reading: { need in
        fetching = true
        output.line("Needs \(Self.needed(need.id, need.range)) — fetching from \(need.source ?? "")")
      })
    let all = previews + fetched.found.map(\.preview)
    defer { for preview in all where preview.temporary { discardBlockStaging(folder: preview.folder) } }
    for error in fetched.unread.values { report(error) }
    if fetching { output.line() }
    for (index, preview) in all.enumerated() {
      if index > 0 { output.line() }
      show(preview)
    }
    guard all.allSatisfy(\.canInstall) else { return .problems }
    let problems = try planBlockInstall(project: project, folders: all.map(\.folder)).problems.map(\.issue)
    if !problems.isEmpty { output.line() }
    for issue in problems { errors.line("jevision: \(describeIssue(issue: issue))") }
    let breaksOnly = problems.allSatisfy { $0.code == "block.dependency.breaks" }
    guard breaksOnly else { return .problems }
    guard request.yes else {
      output.line()
      output.line("Not installed: add --yes to install it.")
      return .problems
    }
    if !problems.isEmpty, !request.force {
      errors.line("jevision: add --force to install it anyway")
      return .problems
    }
    var installing: [BlockPreview] = []
    for preview in all {
      let manifest = preview.manifest
      switch preview.change {
      case .same where !request.force:
        output.line()
        output.line("\(manifest.name) \(manifest.version) is installed already; add --force to install it again.")
        continue
      case .downgrade(let from) where !request.force:
        errors.line("jevision: \(manifest.id) \(from) is installed, which is newer than \(manifest.version); add --force to go back")
        return .problems
      default:
        installing.append(preview)
      }
    }
    guard !installing.isEmpty else { return .ok }
    let blocks = try installBlockSet(
      project: project,
      items: installing.map { BlockInstallItem(folder: $0.folder, source: $0.source, replace: $0.change != .new) },
      force: request.force)
    output.line()
    for block in blocks {
      output.line("Installed \(block.manifest.name) \(block.manifest.version) (\(block.manifest.id)).")
    }
    return .ok
  }

  /// Installs the blocks jevision.blocks.json names and the project hasn't got, each from its address.
  private func restore(in project: String) throws -> ExitStatus {
    let lock = "jevision.blocks.json"
    let items = try restoreBlockSources(project: project)
    let chosen = choose(
      items, id: \.id, needs: { $0.preview?.manifest.dependencies.map(\.id) ?? [] },
      among: "the blocks of \(lock) that aren't installed")
    var ready: [BlockPreview] = []
    var status = ExitStatus.ok
    defer {
      for preview in items.compactMap(\.preview) where preview.temporary && !ready.contains(preview) {
        discardBlockStaging(folder: preview.folder)
      }
    }
    guard let chosen else { return .problems }
    guard !chosen.isEmpty else {
      output.line("Everything in \(lock) is installed already.")
      return .ok
    }
    output.line(chosen.count == 1 ? "1 block of \(lock) isn't installed." : "\(chosen.count) blocks of \(lock) aren't installed.")
    output.line()
    for item in chosen {
      switch item.state {
      case .ready:
        if let preview = item.preview { ready.append(preview) }
      case .versionChanged(let found):
        if request.force, let preview = item.preview {
          ready.append(preview)
          continue
        }
        let kept = Self.needed(item.id, item.lockedVersion ?? "")
        errors.line(
          "jevision: \(lock) keeps \(kept), and \(item.source) now has \(found); skipped — add --force to read what this version does and restore it")
        status = .problems
      case .missing:
        errors.line("jevision: \(item.source) no longer has \(item.id)")
        status = .problems
      case .failed(let issues):
        for issue in issues { errors.line("jevision: \(item.id): \(describeIssue(issue: issue))") }
        status = .problems
      }
    }
    guard !ready.isEmpty else { return .problems }
    return max(status, try install(ready, in: project))
  }

  /// The blocks of `all` that `--only` names, with the blocks of `all` they need, and those need in
  /// turn; all of them when `--only` names none, and `nil` when it names one that isn't `among` them.
  private func choose<Item>(
    _ all: [Item], id: (Item) -> String, needs: (Item) -> [String], among place: String
  ) -> [Item]? {
    guard !request.only.isEmpty else { return all }
    let unknown = request.only.filter { name in !all.contains { id($0) == name } }
    guard unknown.isEmpty else {
      let there = all.isEmpty ? "there are none" : "there are \(all.map(id).joined(separator: ", "))"
      errors.line("jevision: no block \(unknown.joined(separator: ", ")) among \(place); \(there)")
      return nil
    }
    var ids = Set(request.only)
    var next = request.only
    while let current = next.popLast() {
      for need in all.first(where: { id($0) == current }).map(needs) ?? [] where ids.insert(need).inserted {
        next.append(need)
      }
    }
    return all.filter { ids.contains(id($0)) }
  }

  private func report(_ error: any Error) {
    guard case BlockPackageError.Failed(let issues)? = error as? BlockPackageError else {
      errors.line("jevision: \(error.localizedDescription)")
      return
    }
    for issue in issues { errors.line("jevision: \(describeIssue(issue: issue))") }
  }

  private func update(_ id: String?, in project: String) throws -> ExitStatus {
    let ids = id.map { [$0] } ?? listCommunityBlocks(project: project).filter { $0.source != nil }.map(\.manifest.id)
    guard !ids.isEmpty else {
      output.line("No block was installed from an address, so there is nothing to update.")
      return .ok
    }
    var status = ExitStatus.ok
    for id in ids {
      let preview = try updateBlockPackage(project: project, id: id)
      if case .same = preview.change {
        discardBlockStaging(folder: preview.folder)
        output.line("\(preview.manifest.name) \(preview.manifest.version) is the newest version.")
        continue
      }
      status = max(status, try install([preview], in: project))
    }
    return status
  }

  private func remove(_ id: String, in project: String) throws -> ExitStatus {
    let (tests, blocks) = (blockUsage(project: project, id: id), blockDependents(project: project, id: id))
    if !request.force, !tests.isEmpty || !blocks.isEmpty {
      if !tests.isEmpty { errors.line("jevision: \(id) is used by \(tests.joined(separator: ", "))") }
      if !blocks.isEmpty { errors.line("jevision: \(id) is needed by the installed blocks \(blocks.joined(separator: ", "))") }
      errors.line("jevision: add --force to remove it anyway")
      return .problems
    }
    try removeBlockPackage(project: project, id: id)
    output.line("Removed \(id).")
    return .ok
  }

  // MARK: What a block does

  private func show(_ preview: BlockPreview) {
    let manifest = preview.manifest
    output.line("\(manifest.name) \(manifest.version) by \(manifest.author) (\(manifest.license))")
    output.line("  \(manifest.id)")
    if let description = manifest.description, !description.isEmpty { output.line("  \(description)") }
    if !manifest.inputs.isEmpty {
      output.line("  Asks for: \(manifest.inputs.map(\.name).joined(separator: ", "))")
    }
    if !manifest.dependencies.isEmpty { output.line("  Needs: \(Self.needs(manifest))") }
    if preview.source.hasPrefix("https://") { output.line("  From \(preview.source)") }
    switch preview.change {
    case .new: break
    case .same: output.line("  This version is installed already.")
    case .update(let from): output.line("  Version \(from) is installed; this is newer.")
    case .downgrade(let from): output.line("  Version \(from) is installed; this is older.")
    }
    for issue in preview.issues {
      let level = issue.severity == .error ? "Problem" : "Warning"
      errors.line("  \(level): \(describeIssue(issue: issue))")
    }
    guard !preview.steps.isEmpty else { return }
    output.line()
    output.line("What it does:")
    for (index, step) in preview.steps.enumerated() {
      output.line("  \(index + 1). \(step.text)")
    }
    let risky = preview.steps.enumerated().filter { $0.element.risk != nil }
    guard !risky.isEmpty else { return }
    output.line()
    output.line(output.paint("Needs your attention:", .bold))
    for risk in Self.riskOrder {
      let steps = risky.filter { $0.element.risk == risk }
      guard !steps.isEmpty else { continue }
      output.line(output.paint("  \(describeBlockRisk(risk: risk))", .yellow))
      for (index, step) in steps {
        let lines = (step.detail.isEmpty ? step.text : step.detail).split(separator: "\n", omittingEmptySubsequences: false)
        output.line("    step \(index + 1): \(lines[0])")
        for line in lines.dropFirst() { output.line("      \(line)") }
      }
    }
  }

  private static let riskOrder: [BlockRisk] = [.script, .http, .database, .opensApps, .keyboard, .clipboard]

  /// The block `id` with the versions that fit, unless any does: `com.acme.helper ^1.2.0`.
  private static func needed(_ id: String, _ range: String) -> String {
    ["", "*"].contains(range) ? id : "\(id) \(range)"
  }

  /// The blocks `manifest` needs, one after the other.
  private static func needs(_ manifest: BlockManifestInfo) -> String {
    manifest.dependencies.map { needed($0.id, $0.version) }.joined(separator: ", ")
  }
}
