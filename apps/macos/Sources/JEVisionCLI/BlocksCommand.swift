import Foundation
import JEVisionCore

/// What `jevision blocks` was asked to do.
struct BlocksRequest: Equatable {
  enum Action: Equatable {
    case list
    /// A folder, a .zip file or an https address.
    case install(source: String)
    /// The newest version of one installed block, or of every block installed from an address.
    case update(id: String?)
    case remove(id: String)
  }

  var action: Action
  var project: String?
  /// Install or update after showing what the block does.
  var yes = false
  /// Remove a block that tests use, install over a newer version, or install the same version again.
  var force = false
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
      case .install(let source): return try install(try inspectBlockPackage(project: project, source: source), in: project)
      case .update(let id): return try update(id, in: project)
      case .remove(let id): return try remove(id, in: project)
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
      if let source = block.source { output.line("\(String(repeating: " ", count: width + 2))from \(source)") }
    }
    return .ok
  }

  /// Shows `preview` and installs it when `--yes` was given and nothing speaks against it.
  private func install(_ preview: BlockPreview, in project: String) throws -> ExitStatus {
    defer { if preview.temporary { discardBlockStaging(folder: preview.folder) } }
    show(preview)
    guard preview.canInstall else { return .problems }
    guard request.yes else {
      output.line()
      output.line("Not installed: add --yes to install it.")
      return .problems
    }
    switch preview.change {
    case .same:
      guard request.force else {
        output.line()
        output.line("Version \(preview.manifest.version) is installed already; add --force to install it again.")
        return .ok
      }
    case .downgrade(let from):
      guard request.force else {
        errors.line("jevision: version \(from) is installed, which is newer than \(preview.manifest.version); add --force to go back")
        return .problems
      }
    case .new, .update: break
    }
    let replace = preview.change != .new
    let block = try installBlockPackage(project: project, folder: preview.folder, source: preview.source, replace: replace)
    output.line()
    output.line("Installed \(block.manifest.name) \(block.manifest.version) (\(block.manifest.id)).")
    return .ok
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
      status = max(status, try install(preview, in: project))
    }
    return status
  }

  private func remove(_ id: String, in project: String) throws -> ExitStatus {
    let tests = blockUsage(project: project, id: id)
    if !tests.isEmpty, !request.force {
      errors.line("jevision: \(id) is used by \(tests.joined(separator: ", ")); add --force to remove it anyway")
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
}
