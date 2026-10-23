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
      case .reading(let source):
        busy(model.isRestoring ? Text("Fetching the saved blocks…") : Text("Reading the block…"), detail: source)
      case .review: review
      case .installing: busy(Text("Installing…"), detail: model.selectedNames.joined(separator: ", "))
      case .installed(let blocks): installed(blocks)
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
    if model.usesList {
      list
    } else if let preview = model.preview {
      header(preview)
      if let description = preview.manifest.description, !description.isEmpty {
        Text(description)
          .fixedSize(horizontal: false, vertical: true)
      }
      ScrollView { details(preview) }
        .frame(maxHeight: 360)
      reviewButtons(preview.manifest)
    }
  }

  private func header(_ preview: BlockPreview) -> some View {
    let manifest = preview.manifest
    return HStack(alignment: .top, spacing: 12) {
      Image(systemName: manifest.icon ?? ProjectLibrary.defaultBlockSymbol)
        .font(.title)
        .foregroundStyle(.indigo)
        .frame(width: 40)
        .accessibilityHidden(true)
      VStack(alignment: .leading, spacing: 2) {
        Text(manifest.name)
          .font(.title2.bold())
          .accessibilityAddTraits(.isHeader)
        Text("Version \(manifest.version) · \(manifest.license)")
          .foregroundStyle(.secondary)
        Text(manifest.id)
          .font(.caption.monospaced())
          .foregroundStyle(.secondary)
          .textSelection(.enabled)
        BlockOriginView(origin: preview.origin, author: manifest.author)
          .padding(.top, 6)
      }
    }
    .accessibilityElement(children: .combine)
  }

  /// What the package shown does: its problems, the steps worth attention, every step and what it asks for.
  private func details(_ preview: BlockPreview) -> some View {
    VStack(alignment: .leading, spacing: 16) {
      if !model.usesList, let mismatch = model.needMismatch { mismatchBanner(mismatch) }
      if let conflict = model.conflict { conflictBanner(conflict) }
      if model.usesList {
        ForEach(model.errors, id: \.self) { message in
          note(message, symbol: "xmark.octagon.fill", color: .red, label: Text("Problem"))
        }
      } else if let entry = model.focused {
        // One block alone has no row: what stops it, or what it changes for installed blocks, shows here.
        ForEach(model.notes(for: entry), id: \.self) { found in
          note(
            found.text, symbol: Self.symbol(found.kind), color: Self.color(found.kind),
            label: found.kind == .problem ? Text("Problem") : Text("Warning"))
        }
      }
      ForEach(model.warnings, id: \.self) { message in
        note(message, symbol: "exclamationmark.triangle.fill", color: .orange, label: Text("Warning"))
      }
      if !model.riskGroups.isEmpty { attention }
      stepList
      if !preview.manifest.inputs.isEmpty { inputList(preview.manifest.inputs) }
      facts(preview)
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }

  // MARK: Several blocks

  /// The packages to tick on top, what the one picked does below.
  @ViewBuilder
  private var list: some View {
    title(model.isRestoring ? "Restore Blocks" : "Install Blocks")
    if model.isRestoring {
      Text("jevision.blocks.json lists the blocks this project uses and where each comes from. These aren’t installed on this Mac yet.")
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
    }
    if let mismatch = model.needMismatch { mismatchBanner(mismatch) }
    ForEach(model.generalProblems, id: \.self) { message in
      note(message, symbol: "xmark.octagon.fill", color: .red, label: Text("Problem"))
    }
    if model.entries.isEmpty {
      Text("Everything in jevision.blocks.json is installed already.")
        .fixedSize(horizontal: false, vertical: true)
    } else {
      ScrollView {
        VStack(alignment: .leading, spacing: 2) {
          ForEach(model.entries) { row($0) }
        }
      }
      .frame(maxHeight: 200)
      .fixedSize(horizontal: false, vertical: true)
      if let preview = model.preview {
        Divider()
        ScrollView {
          VStack(alignment: .leading, spacing: 16) {
            header(preview)
            if let description = preview.manifest.description, !description.isEmpty {
              Text(description)
                .fixedSize(horizontal: false, vertical: true)
            }
            details(preview)
          }
        }
        .frame(maxHeight: 280)
      }
    }
    listButtons
  }

  private func row(_ entry: BlockInstallEntry) -> some View {
    let selected = model.selection.contains(entry.id)
    let preview = entry.preview
    let badge = BlockInstallModel.badge(entry)
    let notes = model.notes(for: entry)
    return HStack(alignment: .top, spacing: 10) {
      Toggle(isOn: Binding(get: { selected }, set: { _ in model.toggle(entry) })) { Text(entry.name) }
        .toggleStyle(.checkbox)
        .labelsHidden()
        .disabled(!model.canToggle(entry) || model.isBusy)
      Image(systemName: preview?.manifest.icon ?? ProjectLibrary.defaultBlockSymbol)
        .foregroundStyle(.indigo)
        .frame(width: 22)
      VStack(alignment: .leading, spacing: 2) {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
          Text(entry.name).fontWeight(.semibold)
          if let preview {
            Text(verbatim: "\(preview.manifest.version) · \(Self.source(preview))").foregroundStyle(.secondary)
          }
          Spacer(minLength: 0)
          if let badge {
            Text(badge)
              .font(.caption)
              .padding(.horizontal, 6)
              .padding(.vertical, 1)
              .background(.quaternary, in: .capsule)
          }
        }
        if let description = preview?.manifest.description, !description.isEmpty {
          Text(description)
            .font(.callout)
            .foregroundStyle(.secondary)
            .lineLimit(1)
        }
        ForEach(notes, id: \.self) { note in
          Label {
            Text(note.text).fixedSize(horizontal: false, vertical: true)
          } icon: {
            Image(systemName: Self.symbol(note.kind)).foregroundStyle(Self.color(note.kind))
          }
          .font(.caption)
        }
      }
    }
    .padding(8)
    .background(model.focus == entry.id ? Color.accentColor.opacity(0.12) : .clear, in: .rect(cornerRadius: 8))
    .contentShape(.rect)
    .onTapGesture { model.focus = entry.id }
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(Text(Self.label(entry, badge: badge)))
    .accessibilityValue(selected ? Text("Selected") : Text("Not selected"))
    .accessibilityHint(Text(hint(entry, selected: selected, notes: notes)))
    .accessibilityAddTraits(model.focus == entry.id ? .isSelected : [])
    .accessibilityAction { model.toggle(entry) }
    .accessibilityAction(named: Text("Show What It Does")) { model.focus = entry.id }
  }

  private static func label(_ entry: BlockInstallEntry, badge: String?) -> String {
    let words = entry.preview.map {
      String(localized: "\(entry.name), version \($0.manifest.version), \(source($0))")
    } ?? entry.name
    return [words, badge].compactMap(\.self).joined(separator: ", ")
  }

  /// Where the block comes from, in a few words: who made it can only be told by that.
  private static func source(_ preview: BlockPreview) -> String {
    describeBlockOrigin(origin: preview.origin, author: preview.manifest.author).source
  }

  /// Why the checkbox can't be changed, then what the notes under the row say.
  private func hint(_ entry: BlockInstallEntry, selected: Bool, notes: [BlockInstallNote]) -> String {
    var parts: [String] = []
    if !model.canToggle(entry) {
      parts.append(
        selected
          ? String(localized: "Stays selected because another selected block needs it.")
          : String(localized: "Can’t be selected."))
    }
    return (parts + notes.map(\.text)).joined(separator: " ")
  }

  private static func symbol(_ kind: BlockInstallNote.Kind) -> String {
    switch kind {
    case .info: "info.circle"
    case .warning: "exclamationmark.triangle.fill"
    case .problem: "xmark.octagon.fill"
    }
  }

  private static func color(_ kind: BlockInstallNote.Kind) -> Color {
    switch kind {
    case .info: .secondary
    case .warning: .orange
    case .problem: .red
    }
  }

  private var listButtons: some View {
    HStack {
      if !model.isRestoring {
        Button("Choose Another") { model.startOver() }
          .disabled(model.isBusy)
      }
      Spacer()
      Button("Cancel", role: .cancel) { dismiss() }
        .keyboardShortcut(.cancelAction)
      if !model.entries.isEmpty {
        let count = model.selection.count
        if model.isRestoring {
          install(count > 1 ? Text("Restore \(count) Blocks") : Text("Restore"))
        } else {
          install(count > 1 ? Text("Install \(count) Blocks") : Text("Install"))
        }
      }
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

  private func installed(_ blocks: [InstalledBlock]) -> some View {
    VStack(alignment: .leading, spacing: 16) {
      Label {
        Group {
          if let block = blocks.first, blocks.count == 1 {
            Text("“\(block.manifest.name)” is installed")
          } else {
            Text("\(blocks.count) blocks are installed")
          }
        }
        .font(.title2.bold())
      } icon: {
        Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
      }
      .accessibilityAddTraits(.isHeader)
      if blocks.count > 1 {
        VStack(alignment: .leading, spacing: 4) {
          ForEach(blocks, id: \.folder) { block in
            Text(verbatim: "\(block.manifest.name) \(block.manifest.version)")
          }
        }
      }
      (blocks.count == 1
        ? Text("Find it under Community when you add a block to a test.")
        : Text("Find them under Community when you add a block to a test."))
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

/// Offers to install the blocks jevision.blocks.json names and this Mac hasn't got, at the top of the window.
struct BlockRestoreBar: View {
  let count: Int
  let review: @MainActor () -> Void
  let notNow: @MainActor () -> Void

  var body: some View {
    HStack(spacing: 10) {
      Image(systemName: "shippingbox")
        .foregroundStyle(.secondary)
        .font(.title3)
        .accessibilityHidden(true)
      Text("\(count) blocks from jevision.blocks.json aren’t installed on this Mac")
        .fixedSize(horizontal: false, vertical: true)
      Spacer(minLength: 12)
      Button("Not Now", action: notNow)
        .buttonStyle(.glass)
      Button("Review and Install…", action: review)
        .buttonStyle(.glassProminent)
    }
    .padding(.horizontal, 16)
    .padding(.vertical, 12)
    .frame(maxWidth: 720)
    .adaptiveGlass(in: .rect(cornerRadius: 18))
    .padding()
    .accessibilityElement(children: .contain)
    .accessibilityLabel(Text("Blocks to install"))
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
