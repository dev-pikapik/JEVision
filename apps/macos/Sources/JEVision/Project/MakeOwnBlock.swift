import JEVisionCore
import Observation
import SwiftUI

/// Copies an installed block among the project's own blocks, where it can be changed, and, when
/// the person wants, makes the tests that use the installed block use the copy. The installed block
/// stays.
@MainActor
@Observable
final class OwnBlockModel: Identifiable {
  /// The installed block, like `com.acme.sign-in`.
  let id: String
  /// The name of the copy; it starts as the block's name, made free among the project's blocks.
  var name: String
  /// The tests that run the installed block, as files of the project.
  let tests: [String]
  var switchesTests = true
  private(set) var error: String?
  /// The copy, once it is made.
  private(set) var made: MadeOwnBlock?

  private let project: URL
  @ObservationIgnored var onMade: @MainActor (_ file: String) -> Void = { _ in }

  init(project: URL, id: String, name: String) {
    self.project = project
    self.id = id
    tests = blockUsage(project: project.path, id: id)
    let blocks = project.appending(path: ProjectLibrary.blocksFolder)
    self.name = RelativePath.unique(name) { FileManager.default.fileExists(atPath: blocks.appending(path: Self.fileName($0)).path) }
  }

  var canMake: Bool { !name.trimmingCharacters(in: .whitespaces).isEmpty && made == nil }

  /// Makes the copy and switches the tests when asked. A test in `open` that shows changes not in
  /// its file keeps running the installed block, so they aren't lost; the others show the copy.
  func make(open documents: [FlowDocument]) {
    let unsaved = documents.filter(\.hasUnsavedChanges).compactMap { document in
      (try? Data(contentsOf: document.url)).map { (document.url, $0) }
    }
    do {
      let file = try named(makeOwnBlock(project: project.path, id: id))
      var skipped: [String] = []
      if switchesTests && !tests.isEmpty {
        defer {
          for (url, data) in unsaved where (try? Data(contentsOf: url)) != data {
            try? data.write(to: url, options: .atomic)
            skipped.append(RelativePath.from(project, to: url))
          }
          for document in documents where !document.hasUnsavedChanges { document.reload() }
        }
        _ = try switchTestsToCopy(project: project.path, id: id, ownFile: file)
      }
      made = MadeOwnBlock(file: file, skipped: skipped)
      error = nil
      onMade(file)
    } catch let failure as BlockPackageError {
      error = BlockPackageText.message(failure)
    } catch {
      self.error = error.localizedDescription
    }
  }

  /// Gives the copy the name the person chose, in its file name and its steps; returns the file.
  private func named(_ own: OwnBlock) throws -> String {
    let name = self.name.trimmingCharacters(in: .whitespaces)
    let copy = project.appending(path: own.file)
    let blocks = copy.deletingLastPathComponent()
    let url = blocks.appending(
      path: Self.fileName(
        RelativePath.unique(name) {
          let url = blocks.appending(path: Self.fileName($0))
          return url.path != copy.path && FileManager.default.fileExists(atPath: url.path)
        }))
    if url.path == copy.path && name == own.name { return own.file }
    var flow = try loadFlow(json: String(contentsOf: copy, encoding: .utf8))
    flow.name = name
    try saveFlow(flow: flow).write(to: url, atomically: true, encoding: .utf8)
    if url.path != copy.path { try FileManager.default.removeItem(at: copy) }
    return RelativePath.from(project, to: url)
  }

  private static func fileName(_ name: String) -> String {
    ProjectScanner.testName(RelativePath.testFileName(name)) + ProjectLibrary.blockSuffix
  }
}

/// A copy of an installed block among the project's own blocks.
struct MadeOwnBlock: Equatable {
  /// Like `blocks/Sign in.block.json`.
  let file: String
  /// Tests that kept running the installed block because they had changes not in their file.
  let skipped: [String]
}

/// Asks for the name of the copy and whether the tests switch to it.
struct OwnBlockSheet: View {
  @Bindable var model: OwnBlockModel
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    VStack(alignment: .leading, spacing: 16) {
      Text("Make This a Block of the Project")
        .font(.title2.bold())
        .accessibilityAddTraits(.isHeader)
      if let made = model.made {
        done(made)
      } else {
        form
      }
    }
    .padding(20)
    .frame(width: 440)
  }

  private var form: some View {
    VStack(alignment: .leading, spacing: 16) {
      Text("Makes a copy among the project’s own blocks, which you can change. The installed block stays.")
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      Form {
        TextField("Name", text: $model.name)
        if !model.tests.isEmpty {
          Toggle(isOn: $model.switchesTests) {
            Text("Use the copy in the \(model.tests.count) tests that use it")
            Text(CustomBlocksSheet.list(model.tests))
          }
        }
      }
      .formStyle(.grouped)
      .scrollDisabled(true)
      if let error = model.error {
        Text(error)
          .font(.callout)
          .foregroundStyle(.red)
          .fixedSize(horizontal: false, vertical: true)
          .textSelection(.enabled)
      }
      HStack {
        Spacer()
        Button("Cancel", role: .cancel) { dismiss() }
          .keyboardShortcut(.cancelAction)
        Button("Make Block") {
          model.make(open: project.openDocuments)
          if model.made?.skipped.isEmpty == true { dismiss() }
        }
        .keyboardShortcut(.defaultAction)
        .disabled(!model.canMake)
      }
    }
  }

  private func done(_ made: MadeOwnBlock) -> some View {
    VStack(alignment: .leading, spacing: 12) {
      Label {
        Text("The copy is made").font(.headline)
      } icon: {
        Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
      }
      Text("These tests have changes that aren’t saved, so they still use the installed block: \(CustomBlocksSheet.list(made.skipped)).")
        .fixedSize(horizontal: false, vertical: true)
      HStack {
        Spacer()
        Button("Done") { dismiss() }
          .keyboardShortcut(.defaultAction)
      }
    }
  }
}
