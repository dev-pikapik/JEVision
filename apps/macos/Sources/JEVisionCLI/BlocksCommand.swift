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
      errors.line("jevision: " + words("cli.error.noFolder", request.project ?? FileManager.default.currentDirectoryPath))
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
      output.line(words("cli.blocks.none", project))
      return .ok
    }
    let width = blocks.map(\.manifest.id.count).max() ?? 0
    for block in blocks {
      let manifest = block.manifest
      let id = manifest.id.padding(toLength: width, withPad: " ", startingAt: 0)
      output.line(words("cli.blocks.listLine", id, manifest.version, manifest.name, manifest.author))
      let indent = String(repeating: " ", count: width + 2)
      if let source = block.source { output.line(indent + words("cli.blocks.from", source)) }
      if !manifest.dependencies.isEmpty { output.line(indent + words("cli.blocks.needs", Self.needs(manifest))) }
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
      output.line(words("cli.blocks.inSource", packages.count, source))
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
        output.line(words("cli.blocks.fetching", Self.needed(need.id, need.range), need.source ?? ""))
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
      output.line(words("cli.blocks.addYes"))
      return .problems
    }
    if !problems.isEmpty, !request.force {
      errors.line("jevision: " + words("cli.blocks.installForce"))
      return .problems
    }
    var installing: [BlockPreview] = []
    for preview in all {
      let manifest = preview.manifest
      switch preview.change {
      case .same where !request.force:
        output.line()
        output.line(words("cli.blocks.sameVersion", manifest.name, manifest.version))
        continue
      case .downgrade(let from) where !request.force:
        errors.line("jevision: " + words("cli.blocks.downgrade", manifest.id, from, manifest.version))
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
      output.line(words("cli.blocks.installed", block.manifest.name, block.manifest.version, block.manifest.id))
    }
    return .ok
  }

  /// Installs the blocks jevision.blocks.json names and the project hasn't got, each from its address.
  private func restore(in project: String) throws -> ExitStatus {
    let lock = "jevision.blocks.json"
    let items = try restoreBlockSources(project: project)
    let chosen = choose(
      items, id: \.id, needs: { $0.preview?.manifest.dependencies.map(\.id) ?? [] },
      among: words("cli.blocks.restorePlace", lock))
    var ready: [BlockPreview] = []
    var status = ExitStatus.ok
    defer {
      for preview in items.compactMap(\.preview) where preview.temporary && !ready.contains(preview) {
        discardBlockStaging(folder: preview.folder)
      }
    }
    guard let chosen else { return .problems }
    guard !chosen.isEmpty else {
      output.line(words("cli.blocks.restoreNothing", lock))
      return .ok
    }
    output.line(words("cli.blocks.restoreMissing", chosen.count, lock))
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
        errors.line("jevision: " + words("cli.blocks.restoreVersionChanged", lock, kept, item.source, found))
        status = .problems
      case .missing:
        errors.line("jevision: " + words("cli.blocks.restoreGone", item.source, item.id))
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
      let (names, there) = (unknown.joined(separator: ", "), all.map(id).joined(separator: ", "))
      let text = all.isEmpty ? words("cli.blocks.onlyUnknownNone", names, place) : words("cli.blocks.onlyUnknown", names, place, there)
      errors.line("jevision: " + text)
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
      output.line(words("cli.blocks.nothingToUpdate"))
      return .ok
    }
    var status = ExitStatus.ok
    for id in ids {
      let preview = try updateBlockPackage(project: project, id: id)
      if case .same = preview.change {
        discardBlockStaging(folder: preview.folder)
        output.line(words("cli.blocks.newest", preview.manifest.name, preview.manifest.version))
        continue
      }
      status = max(status, try install([preview], in: project))
    }
    return status
  }

  private func remove(_ id: String, in project: String) throws -> ExitStatus {
    let (tests, blocks) = (blockUsage(project: project, id: id), blockDependents(project: project, id: id))
    if !request.force, !tests.isEmpty || !blocks.isEmpty {
      if !tests.isEmpty { errors.line("jevision: " + words("cli.blocks.usedBy", id, tests.joined(separator: ", "))) }
      if !blocks.isEmpty { errors.line("jevision: " + words("cli.blocks.neededBy", id, blocks.joined(separator: ", "))) }
      errors.line("jevision: " + words("cli.blocks.removeForce"))
      return .problems
    }
    try removeBlockPackage(project: project, id: id)
    output.line(words("cli.blocks.removed", id))
    return .ok
  }

  // MARK: What a block does

  private func show(_ preview: BlockPreview) {
    let manifest = preview.manifest
    output.line(words("cli.blocks.title", manifest.name, manifest.version, manifest.author, manifest.license))
    output.line("  \(manifest.id)")
    if let description = manifest.description, !description.isEmpty { output.line("  \(description)") }
    if !manifest.inputs.isEmpty {
      output.line("  " + words("cli.blocks.asksFor", manifest.inputs.map(\.name).joined(separator: ", ")))
    }
    if !manifest.dependencies.isEmpty { output.line("  " + words("cli.blocks.needsBlocks", Self.needs(manifest))) }
    if preview.source.hasPrefix("https://") { output.line("  " + words("cli.blocks.fromAddress", preview.source)) }
    switch preview.change {
    case .new: break
    case .same: output.line("  " + words("cli.blocks.installedAlready"))
    case .update(let from): output.line("  " + words("cli.blocks.newer", from))
    case .downgrade(let from): output.line("  " + words("cli.blocks.older", from))
    }
    for issue in preview.issues {
      let text = describeIssue(issue: issue)
      errors.line("  " + (issue.severity == .error ? words("cli.blocks.problem", text) : words("cli.blocks.warning", text)))
    }
    guard !preview.steps.isEmpty else { return }
    output.line()
    output.line(words("cli.blocks.steps"))
    for (index, step) in preview.steps.enumerated() {
      output.line("  \(index + 1). \(step.text)")
    }
    let risky = preview.steps.enumerated().filter { $0.element.risk != nil }
    guard !risky.isEmpty else { return }
    output.line()
    output.line(output.paint(words("cli.blocks.attention"), .bold))
    for risk in Self.riskOrder {
      let steps = risky.filter { $0.element.risk == risk }
      guard !steps.isEmpty else { continue }
      output.line(output.paint("  \(describeBlockRisk(risk: risk))", .yellow))
      for (index, step) in steps {
        let lines = (step.detail.isEmpty ? step.text : step.detail).split(separator: "\n", omittingEmptySubsequences: false)
        output.line("    " + words("cli.blocks.step", index + 1, lines[0]))
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
