import JEVisionCore
import SwiftUI

/// Saves the selected blocks to the project's library: as a template, whose blocks are copied
/// into a test, or as a custom block with a name, an icon and the fields each use fills in.
struct SaveToLibrarySheet: View {
  let kind: LibraryEntry.Kind
  let document: FlowDocument
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  @State private var name = ""
  @State private var icon = ProjectLibrary.defaultBlockSymbol
  /// Input names of the chosen fields, by `InputChoice.key`.
  @State private var chosen: [String: String] = [:]
  @State private var error: String?

  /// Icons a custom block can have, with what VoiceOver calls them.
  static let icons: [(symbol: String, name: String)] = [
    (ProjectLibrary.defaultBlockSymbol, String(localized: "Box")),
    ("person.badge.key", String(localized: "Sign In")),
    ("person.crop.circle.badge.plus", String(localized: "New Account")),
    ("creditcard", String(localized: "Card")),
    ("cart", String(localized: "Cart")),
    ("envelope", String(localized: "Mail")),
    ("magnifyingglass", String(localized: "Search")),
    ("gearshape", String(localized: "Settings")),
    ("doc.text", String(localized: "Document")),
    ("square.and.arrow.up", String(localized: "Share")),
    ("bell", String(localized: "Notification")),
    ("star", String(localized: "Star")),
  ]

  private var items: [CanvasItem] { document.pickedInOrder }
  private var hasBlocks: Bool { items.contains { $0.blockID != nil } }
  private var canSave: Bool { hasBlocks && !name.trimmingCharacters(in: .whitespaces).isEmpty }

  /// Fields of the selected blocks that can become inputs, each with a name to start from.
  private var fields: [InputChoice] {
    guard let flow = document.flow else { return [] }
    return inputFields(flow: flow, items: items).map { InputChoice(field: $0, flow: flow) }
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 16) {
      (kind == .template ? Text("Save as Template") : Text("Make Block"))
        .font(.title2.bold())
        .accessibilityAddTraits(.isHeader)
      (kind == .template
        ? Text("The selected blocks become a template in the library. Adding it copies them into a test.")
        : Text("The selected blocks become one block in the library. Tests run it as a single step; change it once, and every test that uses it changes."))
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      Form {
        TextField(text: $name, prompt: kind == .template ? Text("Sign in") : Text("Sign in as")) { Text("Name") }
        if kind == .block {
          LabeledContent("Icon") { iconGrid }
          if !fields.isEmpty {
            Section {
              ForEach(fields) { field in inputRow(field) }
            } header: {
              Text("Inputs")
            } footer: {
              Text("Each test that uses the block fills in the chosen fields.")
                .foregroundStyle(.secondary)
            }
          }
        }
      }
      .formStyle(.grouped)
      .scrollDisabled(fields.count < 4)
      if !hasBlocks {
        Text("Select at least one block first.").foregroundStyle(.secondary).font(.callout)
      }
      if let error {
        Text(error).foregroundStyle(.red).font(.callout)
      }
      HStack {
        Spacer()
        Button("Cancel", role: .cancel) { dismiss() }
          .keyboardShortcut(.cancelAction)
        Button("Save", action: save)
          .keyboardShortcut(.defaultAction)
          .disabled(!canSave)
      }
    }
    .padding(20)
    .frame(width: 460, height: kind == .block ? 520 : nil)
  }

  private var iconGrid: some View {
    LazyVGrid(columns: Array(repeating: GridItem(.fixed(30), spacing: 6), count: 6), spacing: 6) {
      ForEach(Self.icons, id: \.symbol) { option in
        Button { icon = option.symbol } label: {
          Image(systemName: option.symbol)
            .frame(width: 28, height: 28)
            .background(icon == option.symbol ? Color.accentColor.opacity(0.25) : .clear, in: .rect(cornerRadius: 6))
        }
        .buttonStyle(.plain)
        .help(Text(option.name))
        .accessibilityLabel(Text(option.name))
        .accessibilityAddTraits(icon == option.symbol ? [.isButton, .isSelected] : .isButton)
      }
    }
  }

  private func inputRow(_ field: InputChoice) -> some View {
    let isOn = Binding(
      get: { chosen[field.key] != nil },
      set: { chosen[field.key] = $0 ? field.suggestedName : nil })
    return VStack(alignment: .leading, spacing: 4) {
      Toggle(isOn: isOn) {
        Text(verbatim: field.title)
        Text(verbatim: field.field.value.isEmpty ? String(localized: "Empty now") : field.field.value)
      }
      if let current = chosen[field.key] {
        TextField(
          text: Binding(get: { current }, set: { chosen[field.key] = $0 }), prompt: Text(verbatim: field.suggestedName)
        ) {
          Text("Input Name")
        }
      }
    }
  }

  private func save() {
    guard let flow = document.flow else { return }
    let text: String
    switch kind {
    case .template:
      text = templateFromCanvas(flow: flow, items: items, name: name)
    case .block:
      let inputs = fields.compactMap { field in
        chosen[field.key].map { BlockInput(block: field.field.block, field: field.field.field, name: $0) }
      }
      text = customBlockFromCanvas(flow: flow, items: items, name: name, icon: icon, inputs: inputs)
    }
    do {
      _ = try project.library.save(text, name: name, as: kind)
      dismiss()
    } catch {
      self.error = ProjectStore.message(error)
    }
  }
}

/// A field that can become an input of a custom block.
struct InputChoice: Identifiable {
  let field: InputField
  /// The block's name and which of its fields this is.
  let title: String
  /// A name for the input, from the block's id: `email`, `cart_file_2`.
  let suggestedName: String

  var key: String { field.block + "/" + field.field }
  var id: String { key }

  init(field: InputField, flow: Flow) {
    self.field = field
    let type = flow.blocks.first { $0.id == field.block }?.kind.typeName ?? ""
    let file = field.field.split(separator: ".").last.flatMap { Int($0) }.map { $0 + 1 }
    if let file {
      title = String(localized: "\(BlockInfo.title(type)): file \(file)")
      suggestedName = "\(field.block)_file_\(file)"
    } else {
      title = String(localized: "\(BlockInfo.title(type)): text")
      suggestedName = field.block
    }
  }
}
