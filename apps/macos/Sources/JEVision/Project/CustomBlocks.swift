import JEVisionCore
import Observation
import SwiftUI

/// What removing an installed block does with the core. The tests give it their own.
struct BlockRemover: Sendable {
  var usage: @Sendable (_ project: String, _ id: String) -> [String]
  var remove: @Sendable (_ project: String, _ id: String) throws -> Void

  static let live = BlockRemover(
    usage: { blockUsage(project: $0, id: $1) },
    remove: { try removeBlockPackage(project: $0, id: $1) })
}

/// A block that tests still use, which the person is asked about before it is removed.
struct BlockRemoval: Identifiable, Equatable {
  let entry: LibraryEntry
  /// The tests that run it, as files of the project.
  let tests: [String]

  var id: String { entry.id }
}

/// Removes installed blocks. A block that tests use is never removed without a word: the tests are
/// named, and the person decides.
@MainActor
@Observable
final class CustomBlocksModel {
  /// Set when the block is used: shown until the person decides.
  var removal: BlockRemoval?
  private(set) var error: String?

  private let project: URL
  private let remover: BlockRemover
  @ObservationIgnored var onChange: @MainActor () -> Void = {}

  init(project: URL, remover: BlockRemover = .live) {
    self.project = project
    self.remover = remover
  }

  /// Removes `entry` when no test uses it, otherwise asks first.
  func remove(_ entry: LibraryEntry) {
    guard let id = entry.package?.id else { return }
    let tests = remover.usage(project.path, id)
    if tests.isEmpty {
      delete(id)
    } else {
      removal = BlockRemoval(entry: entry, tests: tests)
    }
  }

  /// Removes the block of `removal` although tests use it; they say it is missing from then on.
  func confirmRemoval() {
    guard let removal, let id = removal.entry.package?.id else { return }
    self.removal = nil
    delete(id)
  }

  func dismissError() { error = nil }

  private func delete(_ id: String) {
    do {
      try remover.remove(project.path, id)
      error = nil
    } catch let failure as BlockPackageError {
      error = BlockPackageText.message(failure)
    } catch {
      self.error = error.localizedDescription
    }
    onChange()
  }
}

/// Everything about the blocks of the project in one place: the ones from the community, to update
/// or remove, and the project's own, to share.
struct CustomBlocksSheet: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  @State private var model: CustomBlocksModel?
  @State private var installing: BlockInstallModel?
  @State private var sharing: LibraryEntry?
  @State private var owning: OwnBlockModel?
  @State private var showsOwning = false

  private var community: [LibraryEntry] { project.library.blocks.filter { $0.package != nil } }
  private var own: [LibraryEntry] { project.library.blocks.filter { $0.package == nil } }

  var body: some View {
    VStack(alignment: .leading, spacing: 16) {
      Text("Custom Blocks")
        .font(.title2.bold())
        .accessibilityAddTraits(.isHeader)
      List {
        if !project.library.packageIssues.isEmpty {
          Section {
            ForEach(Array(project.library.packageIssues.enumerated()), id: \.offset) { _, issue in
              Label(describeIssue(issue: issue), systemImage: "exclamationmark.triangle")
                .foregroundStyle(issue.severity == .error ? Color.red : Color.orange)
            }
          }
        }
        Section {
          if community.isEmpty {
            Text("No blocks from the community yet. Install one from a folder, a .zip file or an address.")
              .foregroundStyle(.secondary)
          }
          ForEach(community) { entry in communityRow(entry) }
        } header: {
          Text("Community")
        }
        if !own.isEmpty {
          Section {
            ForEach(own) { entry in ownRow(entry) }
          } header: {
            Text("My blocks")
          }
        }
      }
      .frame(minHeight: 240)
      if let error = model?.error {
        Text(error)
          .font(.callout)
          .foregroundStyle(.red)
          .fixedSize(horizontal: false, vertical: true)
          .textSelection(.enabled)
      }
      HStack {
        Button("Install Block…") { installing = project.makeBlockInstaller() }
        Spacer()
        Button("Done") { dismiss() }
          .keyboardShortcut(.defaultAction)
      }
    }
    .padding(20)
    .frame(width: 560)
    .onAppear { model = project.makeCustomBlocks() }
    .sheet(item: $installing) { InstallBlockSheet(model: $0) }
    .sheet(item: $sharing) { entry in
      if let root = project.rootURL { ShareBlockSheet(entry: entry, project: root) }
    }
    // The copy opens in the editor once it is made.
    .sheet(isPresented: $showsOwning, onDismiss: { if owning?.made != nil { dismiss() } }) {
      if let owning { OwnBlockSheet(model: owning) }
    }
    .alert(
      Text("Remove “\(model?.removal?.entry.name ?? "")”?"),
      isPresented: Binding(get: { model?.removal != nil }, set: { if !$0 { model?.removal = nil } }),
      presenting: model?.removal
    ) { _ in
      Button("Remove Anyway", role: .destructive) { model?.confirmRemoval() }
      Button("Keep", role: .cancel) {}
    } message: { removal in
      Text("These tests use it: \(Self.list(removal.tests)). They will stop with a message that the block is missing.")
    }
  }

  /// The tests, by name, as one sentence part.
  static func list(_ tests: [String]) -> String {
    let names = tests.map { ProjectScanner.testName(path: $0) }
    let shown = names.prefix(5).joined(separator: ", ")
    return names.count > 5 ? shown + " " + String(localized: "and \(names.count - 5) more") : shown
  }

  /// The buttons go under the words, so that the name, the whole address and any warning about the
  /// author have the row's width.
  private func communityRow(_ entry: LibraryEntry) -> some View {
    HStack(alignment: .top, spacing: 10) {
      Image(systemName: entry.symbol)
        .foregroundStyle(.indigo)
        .frame(width: 24)
        .accessibilityHidden(true)
      VStack(alignment: .leading, spacing: 1) {
        Text(entry.name).fontWeight(.medium)
        if let package = entry.package {
          Text("Version \(package.version)")
            .font(.callout)
            .foregroundStyle(.secondary)
          BlockOriginView(origin: package.origin, author: package.author, explains: package.source == nil)
          if let source = package.source {
            Text(source)
              .font(.caption)
              .foregroundStyle(.tertiary)
              .fixedSize(horizontal: false, vertical: true)
              .textSelection(.enabled)
          }
        }
        HStack {
          if let url = entry.url, let root = project.rootURL {
            Button("Open") {
              project.selection = RelativePath.from(root, to: url)
              dismiss()
            }
            .help("Shows its steps. To change them, make a copy.")
            .accessibilityLabel(Text("Open \(entry.name)"))
          }
          if let id = entry.package?.id {
            Button("Make a Copy…") {
              owning = project.makeOwnBlockModel(of: id)
              showsOwning = owning != nil
            }
            .help("Makes a copy among the project’s own blocks, which you can change. The installed block stays.")
            .accessibilityLabel(Text("Make a copy of \(entry.name)"))
          }
          if let id = entry.package?.id, entry.package?.source != nil {
            Button("Update") { installing = project.makeBlockInstaller(updating: id) }
              .accessibilityLabel(Text("Update \(entry.name)"))
          }
          Spacer()
          Button("Remove", role: .destructive) { model?.remove(entry) }
            .accessibilityLabel(Text("Remove \(entry.name)"))
        }
        .padding(.top, 6)
      }
    }
    .padding(.vertical, 4)
    .accessibilityElement(children: .contain)
  }

  private func ownRow(_ entry: LibraryEntry) -> some View {
    HStack(spacing: 10) {
      Image(systemName: entry.symbol)
        .foregroundStyle(.indigo)
        .frame(width: 24)
        .accessibilityHidden(true)
      Text(entry.name)
      Spacer()
      Button("Share…") { sharing = entry }
        .accessibilityLabel(Text("Share \(entry.name)"))
    }
    .accessibilityElement(children: .contain)
  }
}
