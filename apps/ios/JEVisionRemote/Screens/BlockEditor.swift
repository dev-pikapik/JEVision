import SwiftUI

/// Makes one change to the test on the Mac; `nil` once the Mac made it.
typealias TestChange = (TestEdit) async -> RemoteError?

/// What the editing sheet of a test shows: a block's settings, or the library to add a block
/// at the end from.
enum EditorSheet: Identifiable, Hashable {
  case block(String)
  case library

  var id: String {
    switch self {
    case .block(let id): "block.\(id)"
    case .library: "library"
    }
  }
}

/// A block's settings and problems, with ways to add a block after it or to delete it. The Mac
/// makes every change; Save sends the settings that differ from what the Mac has.
struct BlockEditor: View {
  let block: TestFlow.Block
  let palette: [TestFlow.PaletteEntry]
  let change: TestChange
  @Environment(\.dismiss) private var dismiss
  @State private var values: [String: String]
  @State private var busy = false
  @State private var error: RemoteError?
  @State private var confirmsDelete = false

  init(block: TestFlow.Block, palette: [TestFlow.PaletteEntry], change: @escaping TestChange) {
    self.block = block
    self.palette = palette
    self.change = change
    _values = State(initialValue: Dictionary(uniqueKeysWithValues: block.fields.map { ($0.path, $0.value) }))
  }

  private var changed: [TestFlow.Field] {
    block.fields.filter { values[$0.path] != $0.value }
  }

  var body: some View {
    NavigationStack {
      Form {
        if !block.problems.isEmpty {
          Section("Problems") {
            ForEach(block.problems, id: \.self) { problem in
              Label {
                Text(BlockText.problem(problem))
              } icon: {
                Image(systemName: problem.warning ? "exclamationmark.triangle.fill" : "xmark.octagon.fill")
                  .foregroundStyle(problem.warning ? .yellow : .red)
              }
            }
          }
        }
        if block.fields.isEmpty {
          Section {
            Text("This block has no settings to change from the iPhone. Change it in JEVision on the Mac.")
              .foregroundStyle(.secondary)
          }
        }
        ForEach(BlockText.sections(block.fields), id: \.fields.first?.path) { section in
          Section {
            ForEach(section.fields) { field in
              FieldRow(field: field, value: binding(field.path))
            }
          } header: {
            if let group = section.group { Text(BlockText.groupTitle(group)) }
          } footer: {
            if let footer = section.group.flatMap(BlockText.groupFooter) { Text(footer) }
          }
        }
        Section {
          NavigationLink {
            BlockLibrary(palette: palette) { type in
              await send(.addBlock(after: block.id, blockType: type))
            }
            .navigationTitle("Add Block After")
          } label: {
            Label("Add Block After This One", systemImage: "plus.rectangle.on.rectangle")
          }
          .disabled(busy || palette.isEmpty)
          Button("Delete Block", systemImage: "trash", role: .destructive) { confirmsDelete = true }
            .disabled(busy)
        }
      }
      .disabled(busy)
      .navigationTitle(BlockInfo.title(block.kind))
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .cancellationAction) {
          Button("Cancel", role: .cancel) { dismiss() }
        }
        ToolbarItem(placement: .confirmationAction) {
          if busy {
            ProgressView()
          } else {
            Button("Save", role: .confirm, action: save)
              .disabled(changed.isEmpty)
          }
        }
      }
      .confirmationDialog("Delete this block?", isPresented: $confirmsDelete, titleVisibility: .visible) {
        Button("Delete Block", role: .destructive) {
          Task { await send(.removeBlock(block.id)) }
        }
      } message: {
        Text("What led to it will lead to the block after it. This can’t be undone.")
      }
      .alert(error?.title ?? "", isPresented: Binding(get: { error != nil }, set: { if !$0 { error = nil } })) {
        Button("OK", role: .cancel) {}
      } message: {
        Text(error?.message ?? "")
      }
    }
  }

  private func binding(_ path: String) -> Binding<String> {
    Binding(get: { values[path] ?? "" }, set: { values[path] = $0 })
  }

  /// Sends each changed setting in turn; one the Mac refuses stops the rest.
  private func save() {
    let edits = changed.map { TestEdit.setField(block: block.id, field: $0.path, value: values[$0.path] ?? "") }
    Task {
      busy = true
      defer { busy = false }
      for edit in edits {
        if let failure = await change(edit) {
          show(failure)
          return
        }
      }
      dismiss()
    }
  }

  /// A change that adds or removes a block: the screen that owns this sheet shows what's next.
  private func send(_ edit: TestEdit) async {
    busy = true
    defer { busy = false }
    if let failure = await change(edit) { show(failure) }
  }

  /// A test that changed on the Mac closes the sheet: the screen under it reads the test again
  /// and says why.
  private func show(_ failure: RemoteError) {
    if failure != .testChanged { error = failure }
  }
}

/// One setting, edited the way its kind is.
private struct FieldRow: View {
  let field: TestFlow.Field
  @Binding var value: String

  var body: some View {
    let label = BlockText.label(field.path)
    switch field.kind {
    case .flag:
      Toggle(label, isOn: Binding(get: { value == "true" }, set: { value = $0 ? "true" : "false" }))
    case .choice:
      Picker(label, selection: $value) {
        if !field.choices.contains(value) { Text(verbatim: value).tag(value) }
        ForEach(field.choices, id: \.self) { Text(BlockText.choice($0)).tag($0) }
      }
    case .longText:
      VStack(alignment: .leading, spacing: 6) {
        Text(label).font(.subheadline).foregroundStyle(.secondary)
        TextField(label, text: $value, axis: .vertical)
          .lineLimit(3...12)
          .font(Self.isCode(field.path) ? .body.monospaced() : .body)
          .textInputAutocapitalization(.never)
          .autocorrectionDisabled(Self.isCode(field.path))
      }
    case .number:
      LabeledContent(label) {
        TextField(label, text: $value, prompt: Text(verbatim: BlockText.placeholder(field)))
          .keyboardType(.decimalPad)
          .multilineTextAlignment(.trailing)
      }
    case .duration, .text:
      VStack(alignment: .leading, spacing: 6) {
        Text(label).font(.subheadline).foregroundStyle(.secondary)
        TextField(label, text: $value, prompt: Text(verbatim: BlockText.placeholder(field)))
          .textInputAutocapitalization(.never)
          .autocorrectionDisabled()
      }
    }
  }

  private static func isCode(_ path: String) -> Bool {
    ["code", "command", "query", "body"].contains(path)
  }
}

/// The blocks a change can add, grouped as the Mac's library groups them.
struct BlockLibrary: View {
  let palette: [TestFlow.PaletteEntry]
  let pick: (String) async -> Void
  @State private var adding: String?

  private var groups: [(group: String, types: [String])] {
    var groups: [(group: String, types: [String])] = []
    for entry in palette {
      if let last = groups.indices.last, groups[last].group == entry.group {
        groups[last].types.append(entry.blockType)
      } else {
        groups.append((entry.group, [entry.blockType]))
      }
    }
    return groups
  }

  var body: some View {
    List {
      ForEach(groups, id: \.group) { group in
        Section(BlockText.libraryGroup(group.group)) {
          ForEach(group.types, id: \.self) { type in
            Button {
              adding = type
              Task {
                await pick(type)
                adding = nil
              }
            } label: {
              HStack {
                Label {
                  Text(BlockInfo.title(type)).foregroundStyle(.primary)
                } icon: {
                  IconTile(symbol: BlockInfo.symbol(type), color: BlockInfo.color(type), size: 30)
                }
                Spacer()
                if adding == type { ProgressView() }
              }
            }
            .disabled(adding != nil)
            .accessibilityIdentifier("library.\(type)")
          }
        }
      }
    }
  }
}
