import JEVisionCore
import SwiftUI

extension ReferenceWizard.Outcome {
  /// “Updated 3 blocks in 2 tests”; the two parts are worded apart so each takes its own plural form.
  var text: String {
    guard blocks > 0 else { return String(localized: "The reference image is replaced.") }
    return String(localized: "Updated \(blocks) blocks") + " " + String(localized: "in \(tests) tests")
  }
}

/// The Accept New Reference wizard: what changed, which blocks use the picture, and each block's
/// settings on the new picture. Nothing is written until the last step.
struct ReferenceWizardView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  let wizard: ReferenceWizard
  let undoManager: UndoManager?
  @State private var picture: ReferencePicture?
  @State private var preview: URL?

  private var images: StepImages { StepImages(step: wizard.step, in: wizard.run) }

  var body: some View {
    VStack(spacing: 0) {
      header
      Divider()
      content
      Divider()
      footer
    }
    .frame(minWidth: 820, idealWidth: 980, minHeight: 560, idealHeight: 700)
    .task { picture = ReferencePicture(url: wizard.screenshot) }
    .quickLookPreview($preview, in: images.all)
    .onChange(of: wizard.page) { _, _ in
      guard wizard.outcome == nil else { return }
      AccessibilityNotification.Announcement(pageAnnouncement).post()
    }
  }

  // MARK: Parts

  private var header: some View {
    HStack(spacing: 12) {
      VStack(alignment: .leading, spacing: 2) {
        Text("Accept New Reference")
          .font(.headline)
          .accessibilityAddTraits(.isHeader)
        Text(wizard.reference)
          .font(.callout)
          .foregroundStyle(.secondary)
          .lineLimit(1)
          .truncationMode(.middle)
      }
      Spacer()
      if wizard.outcome == nil, !wizard.isSingle {
        Text("Step \(wizard.position) of \(wizard.pages.count)")
          .foregroundStyle(.secondary)
          .monospacedDigit()
      }
    }
    .padding(.horizontal, 16)
    .padding(.vertical, 10)
  }

  @ViewBuilder private var content: some View {
    if let outcome = wizard.outcome {
      done(outcome)
    } else {
      switch wizard.page {
      case .compare: compare
      case .affected: affected
      case .block(let index): BlockReview(wizard: wizard, index: index, picture: picture)
      case .finish: finish
      }
    }
  }

  private var footer: some View {
    HStack(spacing: 10) {
      if wizard.outcome == nil {
        Button("Cancel") { dismiss() }
          .keyboardShortcut(.cancelAction)
        Text("Nothing changes until you accept.")
          .font(.callout)
          .foregroundStyle(.secondary)
      }
      Spacer()
      if let failure = wizard.failure {
        Text(failure)
          .font(.callout)
          .foregroundStyle(.red)
          .lineLimit(3)
          .textSelection(.enabled)
          .accessibilityLabel(Text("Can’t replace the reference image: \(failure)"))
      }
      if wizard.outcome != nil {
        Button("Done") { dismiss() }
          .keyboardShortcut(.defaultAction)
          .buttonStyle(.borderedProminent)
      } else {
        if wizard.canGoBack {
          Button("Back") { wizard.back() }
        }
        primaryButtons
      }
    }
    .padding(.horizontal, 16)
    .padding(.vertical, 10)
  }

  @ViewBuilder private var primaryButtons: some View {
    switch wizard.page {
    case .compare where wizard.isSingle, .finish:
      Button("Accept as New Reference", action: accept)
        .keyboardShortcut(.defaultAction)
        .buttonStyle(.borderedProminent)
    case .compare, .affected:
      Button("Continue") { wizard.next() }
        .keyboardShortcut(.defaultAction)
        .buttonStyle(.borderedProminent)
    case .block(let index):
      Button("Skip") { wizard.decide(index, .skipped) }
        .help(Text("Leave this block’s settings as they are"))
      Button("Confirm") { wizard.decide(index, .confirmed) }
        .keyboardShortcut(.defaultAction)
        .buttonStyle(.borderedProminent)
        .help(Text("The settings fit the new picture"))
    }
  }

  // MARK: Pages

  private var compare: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: 14) {
        Comparison(step: wizard.step, images: images) { preview = $0 }
        Text("Next runs compare with it. The current image \(wizard.reference) moves to the Trash, and Edit › Undo puts it back.")
          .font(.callout)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
        if wizard.isSingle, !wizard.entries.isEmpty {
          Divider()
          BlockReview(wizard: wizard, index: 0, picture: picture)
            .frame(height: 420)
        }
      }
      .padding(16)
    }
  }

  private var affected: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: 14) {
        let tests = wizard.entries.reduce(into: [String]()) { if !$0.contains($1.use.test) { $0.append($1.use.test) } }
        Text(String(localized: "Used by \(wizard.entries.count) blocks") + " " + String(localized: "in \(tests.count) tests"))
          .font(.title3.weight(.semibold))
          .accessibilityAddTraits(.isHeader)
        Text("Several tests compare with this picture. Next, check each block against the new one: whether its areas, hidden parts and similarity still fit.")
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
        ForEach(tests, id: \.self) { test in
          VStack(alignment: .leading, spacing: 6) {
            Label(Self.testName(test), systemImage: "doc.text")
              .font(.headline)
              .accessibilityAddTraits(.isHeader)
            ForEach(wizard.entries.filter { $0.use.test == test }) { entry in
              UseRow(entry: entry, isFailed: isFailed(entry)) {
                if isFailed(entry) { Text("Failed this run").foregroundStyle(.red) }
              }
            }
          }
        }
      }
      .padding(16)
      .frame(maxWidth: .infinity, alignment: .leading)
    }
  }

  private var finish: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: 14) {
        Text("Ready to replace the picture")
          .font(.title3.weight(.semibold))
          .accessibilityAddTraits(.isHeader)
        Text("The new screenshot becomes the reference image. The old one moves to the Trash.")
          .foregroundStyle(.secondary)
        ForEach(wizard.entries) { entry in
          UseRow(entry: entry, isFailed: isFailed(entry), showsTest: true) {
            Text(Self.decisionText(entry))
              .foregroundStyle(entry.decision == .skipped ? Color.orange : Color.secondary)
          }
        }
        if wizard.summary.skipped > 0 {
          Label("Skipped blocks keep their settings, and compare with the new picture too.", systemImage: "exclamationmark.triangle")
            .foregroundStyle(.orange)
            .fixedSize(horizontal: false, vertical: true)
        }
      }
      .padding(16)
      .frame(maxWidth: .infinity, alignment: .leading)
    }
  }

  private func done(_ outcome: ReferenceWizard.Outcome) -> some View {
    VStack(spacing: 12) {
      Image(systemName: "checkmark.circle.fill")
        .font(.system(size: 48))
        .foregroundStyle(.green)
        .accessibilityHidden(true)
      Text(outcome.text)
        .font(.title2.weight(.semibold))
        .accessibilityAddTraits(.isHeader)
      if outcome.skipped > 0 {
        Text("Skipped \(outcome.skipped) blocks: they keep their settings, and compare with the new picture too.")
          .foregroundStyle(.orange)
      }
      Text("The old picture is in the Trash. Edit › Undo puts back the picture and the settings.")
        .foregroundStyle(.secondary)
    }
    .multilineTextAlignment(.center)
    .padding(24)
    .frame(maxWidth: .infinity, maxHeight: .infinity)
  }

  // MARK: Actions

  private func accept() {
    do {
      try project.acceptReference(wizard, undoManager: undoManager)
    } catch {
      wizard.failure = ProjectStore.message(error)
      return
    }
    let outcome = wizard.summary
    AccessibilityNotification.Announcement(
      outcome.blocks > 0 ? outcome.text : String(localized: "Done. Undo brings it back.")
    ).post()
    if wizard.isSingle { dismiss() }
  }

  private func isFailed(_ entry: ReferenceWizard.Entry) -> Bool {
    entry.use.test == wizard.step.test && entry.use.block == wizard.step.block
  }

  private var pageAnnouncement: String {
    switch wizard.page {
    case .compare: String(localized: "The old and the new picture")
    case .affected: String(localized: "Blocks that use this picture")
    case .block(let index):
      wizard.entries.indices.contains(index)
        ? String(localized: "Block \(index + 1) of \(wizard.entries.count)") + ", "
          + BlockInfo.title(wizard.entries[index].use.blockType)
        : ""
    case .finish: String(localized: "Ready to replace the picture")
    }
  }

  static func testName(_ file: String) -> String {
    ProjectScanner.testName((file as NSString).lastPathComponent)
  }

  static func decisionText(_ entry: ReferenceWizard.Entry) -> String {
    switch entry.decision {
    case .pending: String(localized: "Not checked")
    case .skipped: String(localized: "Skipped")
    case .confirmed: entry.isEdited ? String(localized: "Confirmed, settings changed") : String(localized: "Confirmed")
    }
  }

  /// What the block does with the picture.
  static func role(_ field: String) -> String {
    switch field {
    case "reference": String(localized: "Compares the screen with it")
    case "until.appears": String(localized: "Waits for it to appear")
    case "until.disappears": String(localized: "Waits for it to disappear")
    case "from": String(localized: "Drags from it")
    case "to": String(localized: "Drags to it")
    default:
      if field.hasPrefix("elseIf.") { String(localized: "Compares the screen with it in another condition") }
      else if field.hasPrefix("inputs.") { String(localized: "Passed to a script") }
      else { String(localized: "Looks for it on the screen") }
    }
  }
}

/// A block that uses the picture: what it is, where and what it does with the picture.
private struct UseRow<Trailing: View>: View {
  let entry: ReferenceWizard.Entry
  var isFailed = false
  var showsTest = false
  @ViewBuilder let trailing: Trailing

  var body: some View {
    HStack(spacing: 10) {
      Image(systemName: BlockInfo.symbol(entry.use.blockType))
        .frame(width: 22)
        .foregroundStyle(.secondary)
        .accessibilityHidden(true)
      VStack(alignment: .leading, spacing: 1) {
        Text(BlockInfo.title(entry.use.blockType))
        Text(showsTest ? ReferenceWizardView.testName(entry.use.test) + " · " + role : role)
          .font(.callout)
          .foregroundStyle(.secondary)
      }
      Spacer()
      trailing
    }
    .padding(.vertical, 4)
    .padding(.horizontal, 10)
    .background(.quaternary.opacity(0.4), in: .rect(cornerRadius: 8))
    .accessibilityElement(children: .combine)
  }

  private var role: String { ReferenceWizardView.role(entry.use.field) }
}

/// One block on the new picture: its areas drawn over it, its settings in words, and a way to change them.
private struct BlockReview: View {
  let wizard: ReferenceWizard
  let index: Int
  let picture: ReferencePicture?
  @State private var tool = AreaKind.ignore
  @State private var selection: AreaID?
  @State private var isEditing = false
  @FocusState private var canvasFocused: Bool

  private var entry: ReferenceWizard.Entry { wizard.entries[index] }

  var body: some View {
    HStack(alignment: .top, spacing: 16) {
      canvas
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: 8))
      details
        .frame(width: 300)
    }
    .padding(16)
    .onChange(of: index) { _, _ in
      isEditing = false
      selection = nil
    }
  }

  @ViewBuilder private var canvas: some View {
    if let picture {
      let label = Text("New reference image, \(picture.image.width) by \(picture.image.height) pixels")
      if isEditing, let settings = entry.settings {
        AreaCanvas(
          picture: picture, reference: settings, tool: tool, selection: $selection, focus: $canvasFocused, label: label
        ) { _, edit in wizard.edit(index, edit) }
      } else {
        ReferenceCanvas(picture: picture, reference: entry.settings ?? Self.bare)
          .aspectRatio(picture.pixels, contentMode: .fit)
          .padding(16)
          .accessibilityElement(children: .ignore)
          .accessibilityLabel(label)
          .accessibilityValue(Text(areasSpoken))
          .accessibilityAddTraits(.isImage)
      }
    } else {
      ContentUnavailableView("The new picture can’t be read", systemImage: "photo.badge.exclamationmark")
    }
  }

  /// A reference with no areas: the picture alone.
  private static let bare = Reference(image: "", compare: .exact, area: nil, ignore: [], hide: [])

  private var areasSpoken: String {
    guard let settings = entry.settings else { return "" }
    return settings.areaIDs.map { id in id.kind.name(id.index) }.joined(separator: ", ")
  }

  private var details: some View {
    Form {
      Section {
        LabeledContent {
          Text(decision)
        } label: {
          Label(BlockInfo.title(entry.use.blockType), systemImage: BlockInfo.symbol(entry.use.blockType))
            .font(.headline)
        }
        Text("in \(ReferenceWizardView.testName(entry.use.test))")
          .foregroundStyle(.secondary)
        Text(ReferenceWizardView.role(entry.use.field))
          .foregroundStyle(.secondary)
      }
      if let settings = entry.settings {
        if isEditing { editing(settings) } else { reading(settings) }
      } else {
        Section {
          Text("This block looks for the picture on the screen, so it has no areas or similarity to check. From now on it looks for the new one.")
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        }
      }
    }
    .formStyle(.grouped)
    .scrollContentBackground(.hidden)
  }

  private var decision: String { ReferenceWizardView.decisionText(entry) }

  private func reading(_ settings: Reference) -> some View {
    Section("Settings") {
      AreaLegend(reference: settings)
      ForEach(settings.areaIDs, id: \.self) { id in
        if let rect = settings[area: id] {
          HStack(spacing: 8) {
            AreaSwatch(kind: id.kind)
            VStack(alignment: .leading, spacing: 1) {
              Text(id.kind.name(id.index))
              Text(Self.place(rect, in: picture))
                .font(.caption)
                .foregroundStyle(.secondary)
                .monospacedDigit()
            }
          }
          .accessibilityElement(children: .combine)
        }
      }
      Text(Self.compareText(settings.compare))
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      HStack {
        Button("Edit Settings") {
          isEditing = true
          canvasFocused = true
        }
        if entry.isEdited {
          Button("Reset") { wizard.reset(index) }
            .help(Text("Put the block’s own settings back"))
        }
      }
    }
  }

  @ViewBuilder private func editing(_ settings: Reference) -> some View {
    Section("Draw") {
      Picker(selection: $tool) {
        ForEach(AreaKind.allCases, id: \.self) { kind in
          Text(kind.title).tag(kind)
        }
      } label: {
        Text("Draw")
      }
      .pickerStyle(.segmented)
      Text(tool.explanation)
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      Button("Add Area", action: addArea)
        .disabled(picture == nil)
    }
    Section("Areas") {
      if settings.area == nil {
        HStack(spacing: 8) {
          AreaSwatch(kind: .compare)
          Text("The whole image is compared")
            .foregroundStyle(.secondary)
        }
      }
      ForEach(settings.areaIDs, id: \.self) { id in
        if let rect = settings[area: id] {
          AreaRow(
            id: id, rect: rect, picture: picture, isSelected: selection == id,
            select: {
              selection = id
              canvasFocused = true
            },
            delete: {
              wizard.edit(index) { $0.remove(id) }
              selection = nil
            })
        }
      }
      if let selection, let rect = settings[area: selection], let picture {
        AreaPixelFields(rect: rect, frame: ImageFrame(pixels: picture.pixels, bounds: picture.pixels)) { new in
          wizard.edit(index) { $0[area: selection] = new }
        }
      }
    }
    Section("Match") {
      if case .similar(let threshold) = settings.compare {
        SimilaritySlider(
          threshold: Binding(
            get: { threshold },
            set: { new in wizard.edit(index) { $0.compare = .similar(threshold: (new * 100).rounded() / 100) } }))
      } else {
        Text(Self.compareText(settings.compare))
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
      HStack {
        Button("Done Editing") { isEditing = false }
        if entry.isEdited {
          Button("Reset") {
            wizard.reset(index)
            selection = nil
          }
        }
      }
    }
  }

  private func addArea() {
    guard let picture else { return }
    let frame = ImageFrame(pixels: picture.pixels, bounds: picture.pixels)
    var added: AreaID?
    wizard.edit(index) { added = $0.add(tool, frame.snapped(Rect(x: 0.35, y: 0.4, width: 0.3, height: 0.2))) }
    selection = added
    canvasFocused = true
  }

  private static func place(_ rect: Rect, in picture: ReferencePicture?) -> String {
    guard let picture else { return "" }
    let pixels = ImageFrame(pixels: picture.pixels, bounds: picture.pixels).pixels(rect)
    return String(localized: "\(pixels.width) × \(pixels.height) px at \(pixels.x), \(pixels.y)")
  }

  private static func compareText(_ compare: Compare) -> String {
    switch compare {
    case .similar(let threshold):
      String(localized: "The share of pixels that must look the same: \(BlockSummary.percent(threshold))")
    case .exact:
      String(localized: "Every pixel must match exactly. Choose Similar in the inspector to let small differences through.")
    case .meaning:
      String(localized: "An AI model decides whether the screen means the same. Hidden areas are covered before it sees the screen.")
    }
  }
}
