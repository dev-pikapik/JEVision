import JEVisionCore
import SwiftUI

/// Installs a block someone shared. It shows what the block does, in plain words, before anything
/// is installed, and says plainly when a step runs a script, reaches the network or the like.
struct InstallBlockSheet: View {
  @Bindable var model: BlockInstallModel
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    VStack(alignment: .leading, spacing: 16) {
      switch model.phase {
      case .choosing: choosing
      case .reading(let source): busy(Text("Reading the block…"), detail: source)
      case .review: review
      case .installing: busy(Text("Installing…"), detail: model.preview?.manifest.name ?? "")
      case .installed(let block): installed(block)
      case .failed(let message): failed(message)
      }
    }
    .padding(20)
    .frame(width: 540)
    .dropDestination(for: URL.self) { urls, _ in
      guard let source = urls.lazy.compactMap(BlockInstallModel.source).first else { return false }
      Task { await model.read(source) }
      return true
    }
    .onDisappear { model.close() }
  }

  // MARK: Choosing

  private var choosing: some View {
    VStack(alignment: .leading, spacing: 16) {
      title("Install a Block")
      if let need = model.need {
        note(
          Text("This test needs the block \(need.label)."), symbol: "puzzlepiece.extension.fill", color: .accentColor,
          label: Text("Needed block"))
      }
      Text("A block is a set of steps someone shared, ready to use in your tests. Choose its folder or .zip file, drop it here, or paste an address: a GitHub repository, a GitHub release or a link to a .zip file.")
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      TextField(text: $model.address, prompt: Text("https://github.com/author/block")) { Text("Address or path") }
        .onSubmit(readAddress)
      Text("Nothing is installed yet: you read what the block does first.")
        .font(.callout)
        .foregroundStyle(.secondary)
      HStack {
        Button("Choose File or Folder…", action: choose)
        Spacer()
        Button("Cancel", role: .cancel) { dismiss() }
          .keyboardShortcut(.cancelAction)
        Button("Read the Block", action: readAddress)
          .keyboardShortcut(.defaultAction)
          .disabled(model.address.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
      }
    }
  }

  private func readAddress() {
    Task { await model.read(model.address) }
  }

  private func choose() {
    let panel = NSOpenPanel()
    panel.canChooseDirectories = true
    panel.canChooseFiles = true
    panel.allowsMultipleSelection = false
    panel.allowedContentTypes = [.folder, .zip]
    panel.message = String(localized: "Choose a block folder or a .zip file")
    panel.prompt = String(localized: "Choose")
    guard panel.runModal() == .OK, let url = panel.url else { return }
    Task { await model.read(url.path) }
  }

  // MARK: Reading and installing

  private func busy(_ text: Text, detail: String) -> some View {
    VStack(spacing: 12) {
      ProgressView()
      text.font(.headline)
      if !detail.isEmpty {
        Text(detail)
          .font(.callout)
          .foregroundStyle(.secondary)
          .lineLimit(2)
          .truncationMode(.middle)
      }
    }
    .frame(maxWidth: .infinity, minHeight: 160)
    .accessibilityElement(children: .combine)
  }

  // MARK: Review

  @ViewBuilder
  private var review: some View {
    if let preview = model.preview {
      let manifest = preview.manifest
      HStack(alignment: .top, spacing: 12) {
        Image(systemName: manifest.icon ?? ProjectLibrary.defaultBlockSymbol)
          .font(.title)
          .foregroundStyle(.indigo)
          .frame(width: 40)
          .accessibilityHidden(true)
        VStack(alignment: .leading, spacing: 2) {
          Text(manifest.name)
            .font(.title2.bold())
            .accessibilityAddTraits(.isHeader)
          Text("Version \(manifest.version) · by \(manifest.author) · \(manifest.license)")
            .foregroundStyle(.secondary)
          Text(manifest.id)
            .font(.caption.monospaced())
            .foregroundStyle(.secondary)
            .textSelection(.enabled)
        }
      }
      .accessibilityElement(children: .combine)
      if let description = manifest.description, !description.isEmpty {
        Text(description)
          .fixedSize(horizontal: false, vertical: true)
      }
      ScrollView {
        VStack(alignment: .leading, spacing: 16) {
          if let mismatch = model.needMismatch { mismatchBanner(mismatch) }
          if let conflict = model.conflict { conflictBanner(conflict) }
          ForEach(model.errors, id: \.self) { message in
            note(message, symbol: "xmark.octagon.fill", color: .red, label: Text("Problem"))
          }
          ForEach(model.warnings, id: \.self) { message in
            note(message, symbol: "exclamationmark.triangle.fill", color: .orange, label: Text("Warning"))
          }
          if !model.riskGroups.isEmpty { attention }
          stepList
          if !manifest.inputs.isEmpty { inputList(manifest.inputs) }
          facts(preview)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
      }
      .frame(maxHeight: 360)
      reviewButtons(manifest)
    }
  }

  @ViewBuilder
  private func mismatchBanner(_ mismatch: BlockNeedMismatch) -> some View {
    switch mismatch {
    case .otherBlock(let wanted):
      note(
        Text("The test needs the block \(wanted), and this is another block."), symbol: "exclamationmark.triangle.fill",
        color: .orange, label: Text("Not the block the test needs"))
    case .otherVersion(let wanted):
      note(
        Text("The test was made with version \(wanted) of this block."), symbol: "info.circle.fill",
        color: .accentColor, label: Text("Another version"))
    }
  }

  private func conflictBanner(_ conflict: BlockConflict) -> some View {
    let text: Text
    switch conflict {
    case .same(let version): text = Text("Version \(version) of this block is installed already.")
    case .update(let from, let to): text = Text("Version \(from) is installed. This is the newer version \(to).")
    case .downgrade(let from, let to): text = Text("Version \(from) is installed. This is the older version \(to).")
    }
    return note(text, symbol: "arrow.triangle.2.circlepath", color: .accentColor, label: Text("Already installed"))
  }

  private func note(_ message: String, symbol: String, color: Color, label: Text) -> some View {
    note(Text(message), symbol: symbol, color: color, label: label)
  }

  private func note(_ message: Text, symbol: String, color: Color, label: Text) -> some View {
    HStack(alignment: .top, spacing: 8) {
      Image(systemName: symbol)
        .foregroundStyle(color)
        .accessibilityHidden(true)
      message.fixedSize(horizontal: false, vertical: true)
      Spacer(minLength: 0)
    }
    .padding(10)
    .background(color.opacity(0.1), in: .rect(cornerRadius: 8))
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(label)
    .accessibilityValue(message)
  }

  /// The steps that run scripts, reach the network or do something else worth knowing before it runs.
  private var attention: some View {
    VStack(alignment: .leading, spacing: 10) {
      Label {
        Text("Needs your attention")
          .font(.headline)
      } icon: {
        Image(systemName: "exclamationmark.shield.fill").foregroundStyle(.orange)
      }
      .accessibilityAddTraits(.isHeader)
      Text("Install a block only from people you trust. These steps do more than click and type in the app under test.")
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      ForEach(model.riskGroups) { group in
        VStack(alignment: .leading, spacing: 6) {
          Label {
            Text(BlockRiskText.title(group.risk)).fontWeight(.semibold)
          } icon: {
            Image(systemName: BlockRiskText.symbol(group.risk))
              .foregroundStyle(group.isSerious ? Color.orange : Color.secondary)
          }
          Text(BlockRiskText.explanation(group.risk))
            .font(.callout)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
          ForEach(group.steps) { step in
            VStack(alignment: .leading, spacing: 2) {
              Text("Step \(step.number): \(step.title)")
                .font(.callout.weight(.medium))
              if !step.detail.isEmpty {
                Text(step.detail)
                  .font(.caption.monospaced())
                  .textSelection(.enabled)
                  .lineLimit(8)
                  .padding(6)
                  .frame(maxWidth: .infinity, alignment: .leading)
                  .background(.quaternary, in: .rect(cornerRadius: 6))
              }
            }
            .accessibilityElement(children: .combine)
          }
        }
        .padding(.leading, 4)
      }
    }
    .padding(12)
    .background(Color.orange.opacity(0.1), in: .rect(cornerRadius: 10))
  }

  private var stepList: some View {
    VStack(alignment: .leading, spacing: 8) {
      Text("What this block does")
        .font(.headline)
        .accessibilityAddTraits(.isHeader)
      ForEach(model.steps) { step in
        HStack(alignment: .firstTextBaseline, spacing: 8) {
          Text("\(step.number).")
            .monospacedDigit()
            .foregroundStyle(.secondary)
            .frame(width: 24, alignment: .trailing)
          VStack(alignment: .leading, spacing: 1) {
            Text(step.title).fontWeight(.medium)
            Text(step.summary)
              .foregroundStyle(.secondary)
              .fixedSize(horizontal: false, vertical: true)
          }
          Spacer(minLength: 0)
          if let risk = step.risk {
            Image(systemName: BlockRiskText.symbol(risk))
              .foregroundStyle(risk == .clipboard ? Color.secondary : Color.orange)
              .help(Text(BlockRiskText.title(risk)))
              .accessibilityLabel(Text(BlockRiskText.title(risk)))
          }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(Text("Step \(step.number): \(step.title). \(step.summary)"))
      }
    }
  }

  private func inputList(_ inputs: [BlockPackageInput]) -> some View {
    VStack(alignment: .leading, spacing: 6) {
      Text("What it asks for")
        .font(.headline)
        .accessibilityAddTraits(.isHeader)
      Text("Each test that uses the block fills these in.")
        .font(.callout)
        .foregroundStyle(.secondary)
      ForEach(inputs, id: \.name) { input in
        HStack(spacing: 6) {
          Text(input.label ?? input.name).fontWeight(.medium)
          if input.label != nil {
            Text(input.name).font(.caption.monospaced()).foregroundStyle(.secondary)
          }
        }
        .accessibilityElement(children: .combine)
      }
    }
  }

  private func facts(_ preview: BlockPreview) -> some View {
    VStack(alignment: .leading, spacing: 2) {
      Text("\(preview.files.count) files · \(BlockPackageText.size(Int64(preview.bytes)))")
      if preview.source.hasPrefix("https://") {
        Text("From \(preview.source)")
          .textSelection(.enabled)
          .lineLimit(2)
          .truncationMode(.middle)
      }
    }
    .font(.caption)
    .foregroundStyle(.secondary)
  }

  private func reviewButtons(_ manifest: BlockManifestInfo) -> some View {
    HStack {
      Button("Choose Another") { model.startOver() }
        .disabled(model.isBusy)
      Spacer()
      Button("Cancel", role: .cancel) { dismiss() }
        .keyboardShortcut(.cancelAction)
      switch model.conflict {
      case .none:
        install(Text("Install"))
      case .same?:
        install(Text("Reinstall"))
      case .update(let from, let to)?:
        Button("Keep Version \(from)") { dismiss() }
        install(Text("Update to \(to)"))
      case .downgrade(let from, let to)?:
        Button("Keep Version \(from)") { dismiss() }
        install(Text("Go Back to \(to)"))
      }
    }
  }

  private func install(_ title: Text) -> some View {
    Button {
      Task { await model.install() }
    } label: {
      title
    }
    .keyboardShortcut(.defaultAction)
    .disabled(!model.canInstall || model.isBusy)
  }

  // MARK: The end

  private func installed(_ block: InstalledBlock) -> some View {
    VStack(alignment: .leading, spacing: 16) {
      Label {
        Text("“\(block.manifest.name)” is installed")
          .font(.title2.bold())
      } icon: {
        Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
      }
      .accessibilityAddTraits(.isHeader)
      Text("Find it under Community when you add a block to a test.")
        .foregroundStyle(.secondary)
      HStack {
        Spacer()
        Button("Done") { dismiss() }
          .keyboardShortcut(.defaultAction)
      }
    }
  }

  private func failed(_ message: String) -> some View {
    VStack(alignment: .leading, spacing: 16) {
      Label {
        Text("The block wasn’t installed")
          .font(.title2.bold())
      } icon: {
        Image(systemName: "xmark.octagon.fill").foregroundStyle(.red)
      }
      .accessibilityAddTraits(.isHeader)
      Text(message)
        .fixedSize(horizontal: false, vertical: true)
        .textSelection(.enabled)
      HStack {
        Spacer()
        Button("Close", role: .cancel) { dismiss() }
          .keyboardShortcut(.cancelAction)
        Button("Try Another") { model.startOver() }
          .keyboardShortcut(.defaultAction)
      }
    }
  }

  private func title(_ key: LocalizedStringKey) -> some View {
    Text(key)
      .font(.title2.bold())
      .accessibilityAddTraits(.isHeader)
  }
}

/// What a kind of risk is called, and what it means, in words anyone follows.
enum BlockRiskText {
  static func title(_ risk: BlockRisk) -> String {
    switch risk {
    case .script: String(localized: "Runs a script on your Mac")
    case .http: String(localized: "Sends requests over the network")
    case .database: String(localized: "Reads or changes a database")
    case .opensApps: String(localized: "Opens apps, files or web pages")
    case .keyboard: String(localized: "Types on the keyboard")
    case .clipboard: String(localized: "Uses the clipboard")
    }
  }

  static func explanation(_ risk: BlockRisk) -> String {
    switch risk {
    case .script: String(localized: "A script can do anything you can do on your Mac, including changing or deleting files.")
    case .http: String(localized: "What the test knows can leave your Mac, and answers from the internet come in.")
    case .database: String(localized: "It opens a database file on your Mac and can read or change what is stored in it.")
    case .opensApps: String(localized: "It starts other apps or opens files and addresses outside the app under test.")
    case .keyboard: String(localized: "The keys go to whatever app has the focus, not only to the app under test.")
    case .clipboard: String(localized: "It replaces or reads what you copied last.")
    }
  }

  static func symbol(_ risk: BlockRisk) -> String {
    switch risk {
    case .script: "terminal"
    case .http: "network"
    case .database: "externaldrive"
    case .opensApps: "arrow.up.forward.app"
    case .keyboard: "keyboard"
    case .clipboard: "doc.on.clipboard"
    }
  }
}
