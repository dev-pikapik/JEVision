import JEVisionCore
import SwiftUI

/// What the person fills in to share one of the project's own blocks.
struct ShareBlockDraft: Equatable {
  var name: String
  var description: String
  var author: String
  var version = "1.0.0"
  var license = "MIT"
  /// The id the person typed; `nil` while it follows the author and the name.
  var customId: String?
  /// The inputs the block asks for, as `${input.NAME}` names.
  let inputs: [String]
  /// What the person installing the block sees for an input instead of its name; empty is left out.
  var labels: [String: String]
  /// The value an input has when a test leaves it out; empty is left out.
  var defaults: [String: String]

  /// The most characters a package takes for the label and for the default value of an input.
  static let labelLimit = 60
  static let defaultLimit = 500

  init(entry: LibraryEntry, author: String = "", license: String = "MIT") {
    name = entry.name
    description = entry.detail
    self.author = author
    self.license = license
    inputs = entry.inputs
    labels = entry.labels
    defaults = entry.defaults
  }

  /// Like `com.acme.sign-in`.
  var id: String { customId ?? suggestBlockId(author: author, name: name) }

  var hasValidId: Bool { isValidBlockId(id: id) }

  /// Everything a package needs is filled in; the core checks the rest.
  var canShare: Bool {
    [name, author, version, license].allSatisfy { !$0.trimmingCharacters(in: .whitespaces).isEmpty } && hasValidId
      && inputs.allSatisfy { !labelTooLong($0) && !defaultTooLong($0) }
  }

  func labelTooLong(_ input: String) -> Bool {
    (Self.clean(labels[input]) ?? "").unicodeScalars.count > Self.labelLimit
  }

  func defaultTooLong(_ input: String) -> Bool {
    (Self.clean(defaults[input]) ?? "").unicodeScalars.count > Self.defaultLimit
  }

  var share: BlockShare {
    func clean(_ text: String) -> String { text.trimmingCharacters(in: .whitespacesAndNewlines) }
    return BlockShare(
      id: id, name: clean(name), description: clean(description), author: clean(author), version: clean(version),
      license: clean(license),
      inputs: inputs.map {
        BlockPackageInput(name: $0, label: Self.clean(labels[$0]), defaultValue: Self.clean(defaults[$0]))
      })
  }

  /// `text` without the spaces around it; `nil` when nothing is left.
  private static func clean(_ text: String?) -> String? {
    text.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.flatMap { $0.isEmpty ? nil : $0 }
  }

  /// The name of the zip file to start from, like `com.acme.sign-in-1.0.0.zip`.
  var archiveName: String { "\(id)-\(version.trimmingCharacters(in: .whitespaces)).zip" }
}

/// Writes one of the project's own blocks as a package others can install: a folder or a zip file.
struct ShareBlockSheet: View {
  let entry: LibraryEntry
  let project: URL
  @Environment(\.dismiss) private var dismiss
  @AppStorage("blockAuthor") private var savedAuthor = ""
  @AppStorage("blockLicense") private var savedLicense = "MIT"
  @State private var draft: ShareBlockDraft
  @State private var asZip = true
  @State private var result: ExportedBlock?
  @State private var error: String?

  init(entry: LibraryEntry, project: URL) {
    self.entry = entry
    self.project = project
    let defaults = UserDefaults.standard
    _draft = State(
      initialValue: ShareBlockDraft(
        entry: entry, author: defaults.string(forKey: "blockAuthor") ?? "",
        license: defaults.string(forKey: "blockLicense") ?? "MIT"))
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 16) {
      Text("Share “\(entry.name)”")
        .font(.title2.bold())
        .accessibilityAddTraits(.isHeader)
      if let result {
        done(result)
      } else {
        form
      }
    }
    .padding(20)
    .frame(width: 480)
  }

  private var form: some View {
    VStack(alignment: .leading, spacing: 16) {
      Text("Makes a package of this block that anyone can install in their JEVision. Only the block’s own steps and pictures go in; nothing else from your project.")
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      Form {
        TextField("Name", text: $draft.name)
        TextField(text: $draft.description, prompt: Text("What it does, in a sentence"), axis: .vertical) { Text("Description") }
        TextField("Author", text: $draft.author)
        TextField("Version", text: $draft.version)
        TextField("License", text: $draft.license)
        TextField(
          text: Binding(get: { draft.id }, set: { draft.customId = $0 }),
          prompt: Text("com.author.name")
        ) { Text("Block id") }
        if !draft.hasValidId {
          Text("Write three or more parts of lowercase letters, digits and - joined by dots, like com.acme.sign-in.")
            .font(.callout)
            .foregroundStyle(.red)
        }
        Picker("Write it as", selection: $asZip) {
          Text("A zip file").tag(true)
          Text("A folder").tag(false)
        }
        .pickerStyle(.segmented)
        if !draft.inputs.isEmpty {
          Section {
            ForEach(draft.inputs, id: \.self) { input in
              LabeledContent {
                HStack {
                  TextField(text: text(\.labels, input), prompt: Text("Label")) { Text("Label of \(input)") }
                  TextField(text: text(\.defaults, input), prompt: Text("Default value")) {
                    Text("Default value of \(input)")
                  }
                }
                .labelsHidden()
              } label: {
                Text(verbatim: input)
              }
              if draft.labelTooLong(input) { tooLong(ShareBlockDraft.labelLimit) }
              if draft.defaultTooLong(input) { tooLong(ShareBlockDraft.defaultLimit) }
            }
          } header: {
            Text("Asks for")
          } footer: {
            Text("People see the label instead of the name. The default value is used when a test leaves the input empty.")
              .foregroundStyle(.secondary)
          }
        }
      }
      .formStyle(.grouped)
      .scrollDisabled(true)
      if let error {
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
        Button("Share…", action: write)
          .keyboardShortcut(.defaultAction)
          .disabled(!draft.canShare)
      }
    }
  }

  private func done(_ result: ExportedBlock) -> some View {
    VStack(alignment: .leading, spacing: 12) {
      Label {
        Text("The block is ready to share")
          .font(.headline)
      } icon: {
        Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
      }
      Text("\(result.files.count) files · \(BlockPackageText.size(Int64(result.bytes)))")
        .foregroundStyle(.secondary)
      ForEach(result.manifest.dependencies, id: \.id) { dependency in
        Label("Needs: \(dependency.id) \(dependency.version)", systemImage: "shippingbox")
          .font(.callout)
          .textSelection(.enabled)
      }
      ForEach(result.warnings, id: \.self) { issue in
        Label(IssueText.message(issue), systemImage: "exclamationmark.triangle.fill")
          .foregroundStyle(.orange)
          .font(.callout)
          .fixedSize(horizontal: false, vertical: true)
      }
      Text("Send it to someone, or put it in a GitHub repository or release. They install it with File → Install Block… or with “jevision blocks install”.")
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      HStack {
        Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: result.path)]) }
        Spacer()
        Button("Done") { dismiss() }
          .keyboardShortcut(.defaultAction)
      }
    }
  }

  /// The text of `input` in `labels` or `defaults` of the draft.
  private func text(_ field: WritableKeyPath<ShareBlockDraft, [String: String]>, _ input: String) -> Binding<String> {
    Binding(get: { draft[keyPath: field][input] ?? "" }, set: { draft[keyPath: field][input] = $0 })
  }

  private func tooLong(_ limit: Int) -> some View {
    Text("Write at most \(limit) characters.")
      .font(.callout)
      .foregroundStyle(.red)
  }

  private func write() {
    guard let destination = chooseDestination() else { return }
    let share = draft.share
    do {
      result = try exportBlockPackage(
        project: project.path, blockFile: entry.id, share: share, destination: destination.path, asZip: asZip)
      error = nil
      savedAuthor = share.author
      savedLicense = share.license
    } catch let failure as BlockPackageError {
      error = BlockPackageText.message(failure)
    } catch {
      self.error = error.localizedDescription
    }
  }

  /// The zip file to write, or the folder to put the block in.
  private func chooseDestination() -> URL? {
    if asZip {
      let panel = NSSavePanel()
      panel.nameFieldStringValue = draft.archiveName
      panel.allowedContentTypes = [.zip]
      panel.message = String(localized: "Choose where to save the block")
      panel.prompt = String(localized: "Save")
      return panel.runModal() == .OK ? panel.url : nil
    }
    let panel = NSOpenPanel()
    panel.canChooseDirectories = true
    panel.canChooseFiles = false
    panel.canCreateDirectories = true
    panel.message = String(localized: "Choose the folder to put the block in")
    panel.prompt = String(localized: "Choose")
    return panel.runModal() == .OK ? panel.url : nil
  }
}
