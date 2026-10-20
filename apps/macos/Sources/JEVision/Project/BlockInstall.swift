import Foundation
import JEVisionCore
import Observation

/// What installing a block does with the core. The tests give it their own.
struct BlockInstaller: Sendable {
  var inspect: @Sendable (_ project: String, _ source: String) throws -> BlockPreview
  var install: @Sendable (_ project: String, _ folder: String, _ source: String, _ replace: Bool) throws -> InstalledBlock
  var update: @Sendable (_ project: String, _ id: String) throws -> BlockPreview
  var discard: @Sendable (_ folder: String) -> Void

  static let live = BlockInstaller(
    inspect: { try inspectBlockPackage(project: $0, source: $1) },
    install: { try installBlockPackage(project: $0, folder: $1, source: $2, replace: $3) },
    update: { try updateBlockPackage(project: $0, id: $1) },
    discard: { discardBlockStaging(folder: $0) })
}

/// One step of a block being looked at before it is installed.
struct BlockInstallStep: Identifiable, Equatable {
  /// From 1, in the order the steps run.
  let number: Int
  let id: String
  /// What kind of block the step is, such as “Click”.
  let title: String
  /// What exactly it does, such as “Click the Sign In button”.
  let summary: String
  let risk: BlockRisk?
  /// The code, address or text of a risky step, as the file says it.
  let detail: String
}

/// The steps that carry the same kind of risk.
struct BlockRiskGroup: Identifiable, Equatable {
  let risk: BlockRisk
  let steps: [BlockInstallStep]

  var id: BlockRisk { risk }

  /// Kinds of risk worth a warning of their own; the clipboard is listed more quietly.
  var isSerious: Bool { risk != .clipboard }
}

/// A block of the same id is installed already.
enum BlockConflict: Equatable {
  case same(version: String)
  case update(from: String, to: String)
  case downgrade(from: String, to: String)
}

/// The sheet that installs a block someone shared: choose where it is, read what it does, then
/// install it. Nothing is installed, and nothing in it runs, before the person says so.
@MainActor
@Observable
final class BlockInstallModel: Identifiable {
  enum Phase {
    /// Waiting for a file, a folder or an address.
    case choosing
    case reading(source: String)
    case review(BlockPreview)
    case installing(BlockPreview)
    case installed(InstalledBlock)
    /// Why the block can't be read or installed, in words.
    case failed(String)
  }

  let id = UUID()
  private(set) var phase = Phase.choosing
  /// What the person typed or pasted: an address of a zip file, a GitHub repository or release.
  var address = ""

  private let project: URL
  private let installer: BlockInstaller
  /// Called once a block is installed, to show it in the library.
  @ObservationIgnored var onInstalled: @MainActor (InstalledBlock) -> Void = { _ in }

  init(project: URL, installer: BlockInstaller = .live) {
    self.project = project
    self.installer = installer
  }

  /// Reads the block at `source` — a folder, a .zip file or an https address — without installing it.
  func read(_ source: String) async {
    let source = source.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !source.isEmpty else { return }
    address = source
    await load(source) { project, installer in try installer.inspect(project, source) }
  }

  /// Reads the newest version of the installed block `id` from the address it came from.
  func readUpdate(of id: String) async {
    await load(id) { project, installer in try installer.update(project, id) }
  }

  private func load(
    _ label: String, _ work: @escaping @Sendable (_ project: String, _ installer: BlockInstaller) throws -> BlockPreview
  ) async {
    discardStaged()
    phase = .reading(source: label)
    let (project, installer) = (project.path, installer)
    let result = await Task.detached { Result { try work(project, installer) } }.value
    // The sheet was closed, or another block chosen, while this one was being read.
    guard case .reading(let current) = phase, current == label else {
      if case .success(let stale) = result, stale.temporary { installer.discard(stale.folder) }
      return
    }
    switch result {
    case .success(let preview): phase = .review(preview)
    case .failure(let error): phase = .failed(Self.message(error))
    }
  }

  /// Installs the block that is being looked at, replacing an installed one of the same id.
  func install() async {
    guard case .review(let preview) = phase, canInstall else { return }
    phase = .installing(preview)
    let (project, installer) = (project.path, installer)
    let replace = conflict != nil
    let result = await Task.detached {
      Result { try installer.install(project, preview.folder, preview.source, replace) }
    }.value
    switch result {
    case .success(let block):
      installer.discard(preview.folder)
      phase = .installed(block)
      onInstalled(block)
    case .failure(let error):
      phase = .failed(Self.message(error))
    }
  }

  /// Back to choosing, with nothing kept from the block that was read.
  func startOver() {
    discardStaged()
    phase = .choosing
  }

  /// Throws away what was unpacked for reading; call it when the sheet closes.
  func close() {
    discardStaged()
    phase = .choosing
  }

  private func discardStaged() {
    switch phase {
    case .review(let preview), .installing(let preview):
      if preview.temporary { installer.discard(preview.folder) }
    default: break
    }
  }

  // MARK: What the sheet shows

  var preview: BlockPreview? {
    switch phase {
    case .review(let preview), .installing(let preview): preview
    default: nil
    }
  }

  var isBusy: Bool {
    switch phase {
    case .reading, .installing: true
    default: false
    }
  }

  /// The block has no error that stops it from being installed.
  var canInstall: Bool { preview?.canInstall == true }

  /// Every step in the order it runs, in the editor's words.
  var steps: [BlockInstallStep] {
    guard let preview else { return [] }
    return preview.steps.enumerated().map { index, step in
      let kind = preview.flow.blocks.first { $0.id == step.block }?.kind
      return BlockInstallStep(
        number: index + 1, id: step.block,
        title: kind.map { BlockInfo.title($0.typeName) } ?? step.block,
        summary: kind.map { BlockSummary.text($0, appName: { AppCatalog.shared.name(of: $0) }, testApp: preview.flow.app) }
          ?? step.text,
        risk: step.risk, detail: step.detail)
    }
  }

  private static let riskOrder: [BlockRisk] = [.script, .http, .database, .opensApps, .keyboard, .clipboard]

  /// The steps that need attention, a group for each kind of risk, the most worrying first.
  var riskGroups: [BlockRiskGroup] {
    let steps = steps
    return Self.riskOrder.compactMap { risk in
      let found = steps.filter { $0.risk == risk }
      return found.isEmpty ? nil : BlockRiskGroup(risk: risk, steps: found)
    }
  }

  /// Set when a block of the same id is installed already.
  var conflict: BlockConflict? {
    guard let preview else { return nil }
    let version = preview.manifest.version
    switch preview.change {
    case .new: return nil
    case .same: return .same(version: version)
    case .update(let from): return .update(from: from, to: version)
    case .downgrade(let from): return .downgrade(from: from, to: version)
    }
  }

  /// Problems that stop the installation, in words.
  var errors: [String] {
    preview?.issues.filter { $0.severity == .error }.map(IssueText.message) ?? []
  }

  /// Things worth a look that don't stop it.
  var warnings: [String] {
    preview?.issues.filter { $0.severity != .error }.map(IssueText.message) ?? []
  }

  /// What to read for a dropped or pasted `url`: a .zip file, a folder that holds a block, or an
  /// https address; `nil` for anything else.
  nonisolated static func source(of url: URL) -> String? {
    guard url.isFileURL else { return url.scheme == "https" ? url.absoluteString : nil }
    var isFolder: ObjCBool = false
    guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isFolder) else { return nil }
    if isFolder.boolValue {
      return FileManager.default.fileExists(atPath: url.appending(path: "block.json").path) ? url.path : nil
    }
    return url.pathExtension.lowercased() == "zip" ? url.path : nil
  }

  private static func message(_ error: Error) -> String {
    if let error = error as? BlockPackageError { return BlockPackageText.message(error) }
    return error.localizedDescription
  }
}
