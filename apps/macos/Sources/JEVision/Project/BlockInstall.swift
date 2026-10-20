import Foundation
import JEVisionCore
import JEVisionHands
import Observation

/// What installing blocks does with the core. The tests give it their own.
struct BlockInstaller: Sendable {
  var inspect: @Sendable (_ project: String, _ source: String) throws -> BlockSourcePreview
  var plan: @Sendable (_ project: String, _ folders: [String]) throws -> BlockInstallPlan
  var install: @Sendable (_ project: String, _ items: [BlockInstallItem], _ force: Bool) throws -> [InstalledBlock]
  var update: @Sendable (_ project: String, _ id: String) throws -> BlockPreview
  var restore: @Sendable (_ project: String) throws -> [RestoreItem]
  var discard: @Sendable (_ folder: String) -> Void

  static let live = BlockInstaller(
    inspect: { try inspectBlockSource(project: $0, source: $1) },
    plan: { try planBlockInstall(project: $0, folders: $1) },
    install: { try installBlockSet(project: $0, items: $1, force: $2) },
    update: { try updateBlockPackage(project: $0, id: $1) },
    restore: { try restoreBlockSources(project: $0) },
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

/// How the block being read differs from the one a test asked for.
enum BlockNeedMismatch: Equatable {
  case otherBlock(wanted: String)
  case otherVersion(wanted: String)
}

/// One package the sheet offers: one of the source, one another package needs, or one that
/// jevision.blocks.json names.
struct BlockInstallEntry: Identifiable, Equatable {
  /// The package as read; `nil` for one of jevision.blocks.json its address didn't give.
  let preview: BlockPreview?
  /// Like `com.acme.sign-in`.
  let packageID: String
  /// The address or path it comes from.
  let source: String
  /// The name of the package it was fetched for, when another package needs it.
  var neededBy: String?
  /// Where it stands at its address, when the sheet restores jevision.blocks.json.
  var restore: RestoreState?
  /// The version jevision.blocks.json keeps; `nil` when it keeps none.
  var lockedVersion: String?

  init(preview: BlockPreview, neededBy: String? = nil) {
    self.preview = preview
    packageID = preview.manifest.id
    source = preview.source
    self.neededBy = neededBy
  }

  init(restoring item: RestoreItem) {
    preview = item.preview
    packageID = item.id
    source = item.source
    restore = item.state
    lockedVersion = item.lockedVersion
  }

  var id: String { preview?.folder ?? "\(source)#\(packageID)" }
  var name: String { preview?.manifest.name ?? packageID }
}

/// Something the list says about a package, under its name.
struct BlockInstallNote: Hashable {
  enum Kind { case info, warning, problem }
  let kind: Kind
  let text: String
}

/// The sheet that installs blocks someone shared: choose where they are, read what each does, then
/// install the ones ticked. Nothing is installed, and nothing in them runs, before the person says so.
@MainActor
@Observable
final class BlockInstallModel: Identifiable {
  enum Phase {
    /// Waiting for a file, a folder or an address.
    case choosing
    case reading(source: String)
    case review
    case installing
    case installed([InstalledBlock])
    /// Why the blocks can't be read or installed, in words.
    case failed(String)
  }

  let id = UUID()
  private(set) var phase = Phase.choosing
  /// What the person typed or pasted: an address of a zip file, a GitHub repository or release.
  var address = ""
  /// The block a test asked for, when the sheet was opened from the error about it.
  var need: BlockNeed?
  /// The packages read, those they need after them.
  private(set) var entries: [BlockInstallEntry] = []
  /// The entries to install, by id.
  private(set) var selection: Set<String> = []
  /// The entry whose steps show below the list.
  var focus: String?
  /// How the ticked entries go in, worked out again on every change.
  private(set) var plan: BlockInstallPlan?
  /// The plan for every entry that has no error: what stops one that isn't ticked.
  private var fullPlan: BlockInstallPlan?
  /// Addresses of needed packages that couldn't be read, with why.
  private(set) var unread: [String: String] = [:]
  /// The entries are the blocks jevision.blocks.json names and the project hasn't got.
  private(set) var isRestoring = false

  private let project: URL
  private let installer: BlockInstaller
  /// Called once blocks are installed, to show them in the library.
  @ObservationIgnored var onInstalled: @MainActor ([InstalledBlock]) -> Void = { _ in }

  init(project: URL, installer: BlockInstaller = .live) {
    self.project = project
    self.installer = installer
  }

  /// Reads the blocks at `source` — a folder, a .zip file or an https address — without installing them.
  func read(_ source: String) async {
    let source = source.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !source.isEmpty else { return }
    address = source
    await load(source) { project, installer in
      try installer.inspect(project, source).packages.map { BlockInstallEntry(preview: $0) }
    }
  }

  /// Reads the newest version of the installed block `id` from the address it came from.
  func readUpdate(of id: String) async {
    await load(id) { project, installer in [BlockInstallEntry(preview: try installer.update(project, id))] }
  }

  /// Reads the blocks jevision.blocks.json names and the project hasn't got, each from its address.
  func restore() async {
    await load(Self.lockFile, restoring: true) { project, installer in
      try installer.restore(project).map(BlockInstallEntry.init(restoring:))
    }
  }

  static let lockFile = "jevision.blocks.json"

  private func load(
    _ label: String, restoring: Bool = false,
    _ work: @escaping @Sendable (_ project: String, _ installer: BlockInstaller) throws -> [BlockInstallEntry]
  ) async {
    discardStaged()
    (entries, selection, focus, plan, fullPlan, unread) = ([], [], nil, nil, nil, [:])
    isRestoring = restoring
    phase = .reading(source: label)
    let (project, installer) = (project.path, installer)
    let result = await Task.detached {
      Result { Self.withDependencies(try work(project, installer), project: project, installer: installer) }
    }.value
    // The sheet was closed, or other blocks chosen, while these were being read.
    guard case .reading(let current) = phase, current == label else {
      if case .success(let stale) = result { Self.discard(stale.entries, installer) }
      return
    }
    switch result {
    case .success(let read):
      entries = read.entries
      unread = read.unread.mapValues(Self.message)
      fullPlan = try? installer.plan(project, entries.filter { $0.preview?.canInstall == true }.compactMap(\.preview?.folder))
      selection = Set(entries.filter { usesList ? isWanted($0) : canSelect($0) }.map(\.id))
      let needed = need.flatMap { need in entries.first { $0.packageID == need.id } }
      if let needed, !selection.contains(needed.id), canSelect(needed) { tick(needed) }
      focus = (needed ?? entries.first)?.id
      phase = .review
      replan()
    case .failure(let error):
      phase = .failed(Self.message(error))
    }
  }

  /// `entries` with the packages they need that the project hasn't got, read from the address each names.
  private nonisolated static func withDependencies(
    _ entries: [BlockInstallEntry], project: String, installer: BlockInstaller
  ) -> (entries: [BlockInstallEntry], unread: [String: any Error]) {
    let fetched = BlockDependencyFetch.fetch(
      for: entries.compactMap(\.preview), plan: { try installer.plan(project, $0) },
      inspect: { try installer.inspect(project, $0) }, discard: installer.discard)
    return (entries + fetched.found.map { BlockInstallEntry(preview: $0.preview, neededBy: $0.neededBy) }, fetched.unread)
  }

  /// Ticks `entry`, or leaves it out unless a ticked package needs it.
  func toggle(_ entry: BlockInstallEntry) {
    if selection.contains(entry.id) {
      guard requiredBy(entry).isEmpty else { return }
      selection.remove(entry.id)
    } else {
      guard canSelect(entry) else { return }
      tick(entry)
    }
    replan()
  }

  private func tick(_ entry: BlockInstallEntry) {
    selection.insert(entry.id)
    selection.formUnion(alsoInstalls(entry).map(\.id))
  }

  private func replan() {
    let folders = entries.filter { selection.contains($0.id) }.compactMap(\.preview?.folder)
    guard !folders.isEmpty else {
      plan = nil
      return
    }
    do {
      plan = try installer.plan(project.path, folders)
    } catch {
      plan = nil
      phase = .failed(Self.message(error))
    }
  }

  /// Installs the ticked blocks, what another needs first, in place of installed ones of the same ids.
  func install() async {
    guard case .review = phase, canInstall else { return }
    let items = entries.filter { selection.contains($0.id) }.compactMap(\.preview).map {
      BlockInstallItem(folder: $0.folder, source: $0.source, replace: $0.change != .new)
    }
    // Only the warnings that an installed block needs another version are left: the person read them.
    let force = plan?.problems.isEmpty == false
    phase = .installing
    let (project, installer) = (project.path, installer)
    let result = await Task.detached { Result { try installer.install(project, items, force) } }.value
    switch result {
    case .success(let blocks):
      discardStaged()
      entries = []
      phase = .installed(blocks)
      onInstalled(blocks)
    case .failure(let error):
      phase = .failed(Self.message(error))
    }
  }

  /// Back to choosing, with nothing kept from the blocks that were read.
  func startOver() {
    discardStaged()
    entries = []
    phase = .choosing
  }

  /// Throws away what was unpacked for reading; call it when the sheet closes.
  func close() {
    startOver()
  }

  private func discardStaged() {
    Self.discard(entries, installer)
  }

  private nonisolated static func discard(_ entries: [BlockInstallEntry], _ installer: BlockInstaller) {
    for preview in entries.compactMap(\.preview) where preview.temporary { installer.discard(preview.folder) }
  }

  // MARK: The list

  /// Several packages, or the ones to restore, are listed to tick; one package alone is shown as it is.
  var usesList: Bool { isRestoring || entries.count > 1 }

  var focused: BlockInstallEntry? { entries.first { $0.id == focus } ?? entries.first }

  /// The entry has no error, its address gave it, and nothing it needs is missing.
  func canSelect(_ entry: BlockInstallEntry) -> Bool {
    guard entry.preview?.canInstall == true else { return false }
    let stops = Set(["block.dependency.missing", "block.dependency.unmet"])
    return !(fullPlan?.problems ?? []).contains { $0.folder == entry.id && stops.contains($0.issue.code) }
  }

  /// Ticked from the start: new blocks and updates, or what jevision.blocks.json keeps, unless an
  /// installed block needs another version of it.
  private func isWanted(_ entry: BlockInstallEntry) -> Bool {
    guard canSelect(entry), !(fullPlan?.problems ?? []).contains(where: { $0.folder == entry.id }) else { return false }
    switch (entry.restore, entry.preview?.change) {
    case (.ready?, _), (nil, .new?), (nil, .update?): return true
    default: return false
    }
  }

  /// The listed packages `entry` needs, and those need in turn, that go in with it when it's ticked.
  func alsoInstalls(_ entry: BlockInstallEntry) -> [BlockInstallEntry] {
    var found: [BlockInstallEntry] = []
    var next = [entry]
    while let current = next.popLast() {
      for need in current.preview?.manifest.dependencies ?? [] {
        guard let other = entries.first(where: { $0.packageID == need.id }), other.id != entry.id,
          !found.contains(other), isWanted(other)
        else { continue }
        found.append(other)
        next.append(other)
      }
    }
    return found
  }

  /// The names of the ticked packages that can't go in without `entry`, which stays ticked for them.
  func requiredBy(_ entry: BlockInstallEntry) -> [String] {
    guard selection.contains(entry.id) else { return [] }
    return entries.filter { other in
      other.id != entry.id && selection.contains(other.id) && alsoInstalls(other).contains(entry)
    }.map(\.name)
  }

  /// The checkbox of `entry` can be changed.
  func canToggle(_ entry: BlockInstallEntry) -> Bool {
    selection.contains(entry.id) ? requiredBy(entry).isEmpty : canSelect(entry)
  }

  /// What stops the ticked packages and isn't about one of them, such as packages that need each other.
  var generalProblems: [String] {
    (plan?.problems ?? []).filter { $0.folder == nil }.map { IssueText.message($0.issue) }
  }

  /// What the list says under `entry`: where it comes from, what goes in with it, what stops it.
  func notes(for entry: BlockInstallEntry) -> [BlockInstallNote] {
    var notes: [BlockInstallNote] = []
    let required = requiredBy(entry)
    if let by = entry.neededBy {
      notes.append(.init(kind: .info, text: String(localized: "Needed by \(by). From \(entry.source)")))
    } else if !required.isEmpty {
      notes.append(.init(kind: .info, text: String(localized: "Needed by \(required.joined(separator: ", "))")))
    }
    let also = alsoInstalls(entry).map(\.name)
    if !also.isEmpty {
      notes.append(.init(kind: .info, text: String(localized: "Also installs \(also.joined(separator: ", "))")))
    }
    switch entry.restore {
    case .versionChanged(let found)?:
      let saved = entry.lockedVersion ?? ""
      notes.append(.init(
        kind: .warning,
        text: String(localized: "Saved \(saved), the address now has \(found) — read what it does before installing")))
    case .missing?:
      notes.append(.init(kind: .problem, text: String(localized: "\(entry.source) no longer has this block.")))
    case .failed(let issues)?:
      notes += issues.map { .init(kind: .problem, text: IssueText.message($0)) }
    case .ready?, nil:
      break
    }
    notes += (entry.preview?.issues ?? []).filter { $0.severity == .error }.map { .init(kind: .problem, text: IssueText.message($0)) }
    let stops = selection.contains(entry.id) ? plan : fullPlan
    for problem in stops?.problems ?? [] where problem.folder == entry.id {
      var text = IssueText.message(problem.issue)
      if let source = problem.issue.param("source"), let why = unread[source] { text += " " + why }
      notes.append(.init(kind: problem.issue.code == "block.dependency.breaks" ? .warning : .problem, text: text))
    }
    return notes
  }

  /// What the version of `entry` is to the one installed, in a word; `nil` when there's nothing to compare.
  static func badge(_ entry: BlockInstallEntry) -> String? {
    switch entry.preview?.change {
    case .new?: String(localized: "New")
    case .update?: String(localized: "Newer Version")
    case .same?: String(localized: "Same Version")
    case .downgrade?: String(localized: "Older Version")
    case nil: nil
    }
  }

  // MARK: What the sheet shows

  /// The package whose details are shown.
  var preview: BlockPreview? { focused?.preview }

  var isBusy: Bool {
    switch phase {
    case .reading, .installing: true
    default: false
    }
  }

  /// Something is ticked and nothing stops it from being installed, apart from installed blocks
  /// that need another version of one of them, which the person is told about.
  var canInstall: Bool {
    let chosen = entries.filter { selection.contains($0.id) }
    guard !chosen.isEmpty, let plan else { return false }
    return chosen.allSatisfy { $0.preview?.canInstall == true }
      && plan.problems.allSatisfy { $0.issue.code == "block.dependency.breaks" }
  }

  /// The names of the ticked blocks.
  var selectedNames: [String] { entries.filter { selection.contains($0.id) }.map(\.name) }

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

  /// Set when the blocks being read don't hold the one the test asked for.
  var needMismatch: BlockNeedMismatch? {
    guard let need, !entries.isEmpty else { return nil }
    guard let manifest = entries.first(where: { $0.packageID == need.id })?.preview?.manifest else {
      return .otherBlock(wanted: need.label)
    }
    if !need.version.isEmpty, manifest.version != need.version { return .otherVersion(wanted: need.version) }
    return nil
  }

  /// Problems that stop the installation of the package shown, in words.
  var errors: [String] {
    preview?.issues.filter { $0.severity == .error }.map(IssueText.message) ?? []
  }

  /// Things worth a look that don't stop it.
  var warnings: [String] {
    preview?.issues.filter { $0.severity != .error }.map(IssueText.message) ?? []
  }

  /// What to read for a dropped or pasted `url`: a .zip file, a folder that holds blocks, or an
  /// https address; `nil` for anything else.
  nonisolated static func source(of url: URL) -> String? {
    guard url.isFileURL else { return url.scheme == "https" ? url.absoluteString : nil }
    var isFolder: ObjCBool = false
    guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isFolder) else { return nil }
    if isFolder.boolValue {
      let inside = (try? FileManager.default.contentsOfDirectory(atPath: url.path)) ?? []
      return inside.contains { $0 == "block.json" || $0.hasSuffix(".jevblock") } ? url.path : nil
    }
    return url.pathExtension.lowercased() == "zip" ? url.path : nil
  }

  private nonisolated static func message(_ error: any Error) -> String {
    if let error = error as? BlockPackageError { return BlockPackageText.message(error) }
    return error.localizedDescription
  }
}
