import JEVisionCore
import SwiftUI
import UniformTypeIdentifiers

/// What the palette adds.
enum PaletteChoice: Hashable {
  case block(String)
  case note
  case frame
  /// A template or a custom block of the project.
  case library(LibraryEntry)

  var title: String {
    switch self {
    case .block(let type): BlockInfo.title(type)
    case .note: String(localized: "Note")
    case .frame: String(localized: "Frame")
    case .library(let entry): entry.name
    }
  }

  /// One line under the title.
  var tagline: String {
    switch self {
    case .block(let type): BlockInfo.tagline(type)
    case .note: String(localized: "A sticky note for whoever reads the test")
    case .frame: String(localized: "Groups blocks into a section with a title")
    case .library(let entry):
      entry.kind == .template
        ? String(localized: "Template: adds its blocks to the test")
        : String(localized: "Custom block: runs its steps as one")
    }
  }

  var symbol: String {
    switch self {
    case .block(let type): BlockInfo.symbol(type)
    case .note: "note.text"
    case .frame: "rectangle.dashed"
    case .library(let entry): entry.symbol
    }
  }

  var color: Color {
    switch self {
    case .block(let type): BlockInfo.color(ofType: type)
    case .note: .yellow
    case .frame: .gray
    case .library(let entry): entry.kind == .template ? .teal : .indigo
    }
  }

  /// The group the palette lists the choice under.
  var groupTitle: String {
    switch self {
    case .block(let type): BlockInfo.catalog.first { $0.typeName == type }.map { BlockInfo.groupTitle($0.group) } ?? ""
    case .note, .frame: String(localized: "Canvas")
    case .library: String(localized: "Library")
    }
  }

  /// Every block type in palette order, then a note and a frame.
  static let all: [PaletteChoice] = BlockInfo.catalog.map { .block($0.typeName) } + [.note, .frame]

  /// `all`, then the project's templates and custom blocks.
  static func all(with library: [LibraryEntry]) -> [PaletteChoice] {
    all + library.map(PaletteChoice.library)
  }

  /// `choices` matching `query`, best first; all of them for an empty query.
  static func search(_ query: String, in choices: [PaletteChoice] = all) -> [PaletteChoice] {
    let candidates = choices.map { choice in
      var keywords = [choice.tagline, choice.groupTitle]
      switch choice {
      case .block(let type): keywords += [BlockInfo.explanation(type), type]
      case .library(let entry): keywords += [entry.detail, String(localized: "template block library")]
      case .note, .frame: break
      }
      return SearchCandidate(title: choice.title, keywords: keywords.joined(separator: " "))
    }
    return rankSearch(query: query, candidates: candidates).map { choices[Int($0)] }
  }

  /// `choices` in runs of the same group, in their order.
  static func grouped(_ choices: [PaletteChoice] = all) -> [(title: String, choices: [PaletteChoice])] {
    var result: [(title: String, choices: [PaletteChoice])] = []
    for choice in choices {
      if result.last?.title == choice.groupTitle {
        result[result.count - 1].choices.append(choice)
      } else {
        result.append((choice.groupTitle, [choice]))
      }
    }
    return result
  }
}

extension UTType {
  /// A block type dragged from the palette onto the canvas.
  static let jevisionBlock = UTType(exportedAs: "io.github.dev-pikapik.jevision.block")
}

/// A block type dragged from the palette onto the canvas.
struct BlockDrag: Codable, Transferable {
  let type: String

  static var transferRepresentation: some TransferRepresentation {
    CodableRepresentation(contentType: .jevisionBlock)
  }
}

/// The library of blocks: grouped by what they do, searchable, every block with its icon and one
/// line on what it does. ↑↓ move, Return adds, Escape closes; a block can also be dragged onto
/// the canvas.
struct PaletteView: View {
  static let size = CGSize(width: 400, height: 460)
  /// The project's templates and custom blocks, listed after the blocks.
  var library: [LibraryEntry] = []
  let pick: (PaletteChoice) -> Void
  let close: () -> Void
  @State private var query = ""
  @State private var highlighted: PaletteChoice?
  @FocusState private var searchFocused: Bool

  private var isSearching: Bool { !query.trimmingCharacters(in: .whitespaces).isEmpty }

  /// The choices in the order the rows are drawn: best match first while searching, by group otherwise.
  private var choices: [PaletteChoice] {
    let all = PaletteChoice.all(with: library)
    return isSearching ? PaletteChoice.search(query, in: all) : all
  }

  var body: some View {
    VStack(spacing: 0) {
      HStack {
        Image(systemName: "magnifyingglass").foregroundStyle(.secondary).accessibilityHidden(true)
        TextField(text: $query, prompt: Text("Add a block")) { Text("Search blocks") }
          .textFieldStyle(.plain)
          .font(.title3)
          .focused($searchFocused)
          .onSubmit(submit)
      }
      .padding(12)
      Divider()
      ScrollViewReader { proxy in
        List(selection: $highlighted) {
          if isSearching {
            ForEach(choices, id: \.self, content: row)
          } else {
            ForEach(PaletteChoice.grouped(choices), id: \.title) { group in
              Section(group.title) {
                ForEach(group.choices, id: \.self, content: row)
              }
            }
          }
        }
        .listStyle(.sidebar)
        .scrollContentBackground(.hidden)
        .overlay {
          if choices.isEmpty { ContentUnavailableView.search(text: query) }
        }
        .onChange(of: highlighted) { if let highlighted { proxy.scrollTo(highlighted) } }
      }
      Divider()
      Text("Return adds after the selected block. Drag a block onto the canvas to place it.")
        .font(.caption)
        .foregroundStyle(.secondary)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }
    .frame(width: Self.size.width, height: Self.size.height)
    .adaptiveGlass(in: .rect(cornerRadius: 20))
    .onAppear {
      searchFocused = true
      highlighted = choices.first
    }
    .onChange(of: query) { highlighted = choices.first }
    .onKeyPress(.downArrow) { move(1) }
    .onKeyPress(.upArrow) { move(-1) }
    .onKeyPress(.escape) {
      close()
      return .handled
    }
    .accessibilityElement(children: .contain)
    .accessibilityLabel(Text("Add a block"))
    .accessibilityAddTraits(.isModal)
  }

  private func row(_ choice: PaletteChoice) -> some View {
    PaletteRow(symbol: choice.symbol, color: choice.color, title: Text(choice.title), detail: Text(choice.tagline))
      .tag(choice)
      .id(choice)
      .contentShape(.rect)
      .onTapGesture { pick(choice) }
      .help(Text(choice.help))
      .modifier(BlockDragSource(choice: choice))
      .accessibilityAddTraits(.isButton)
      .accessibilityAction { pick(choice) }
  }

  private func move(_ step: Int) -> KeyPress.Result {
    let ids = choices
    guard !ids.isEmpty else { return .handled }
    let index = highlighted.flatMap(ids.firstIndex) ?? (step > 0 ? -1 : ids.count)
    highlighted = ids[min(max(index + step, 0), ids.count - 1)]
    return .handled
  }

  private func submit() {
    if let choice = highlighted ?? choices.first { pick(choice) }
  }
}

extension PaletteChoice {
  /// The full description, shown when the pointer rests on the row.
  fileprivate var help: String {
    switch self {
    case .block(let type): BlockInfo.explanation(type)
    case .library(let entry) where !entry.detail.isEmpty: entry.detail
    default: tagline
    }
  }
}

/// Blocks drag onto the canvas; a note and a frame are only added from the list.
private struct BlockDragSource: ViewModifier {
  let choice: PaletteChoice

  func body(content: Content) -> some View {
    if case .block(let type) = choice {
      content.draggable(BlockDrag(type: type)) {
        Label(choice.title, systemImage: choice.symbol)
          .padding(8)
          .background(.regularMaterial, in: .rect(cornerRadius: 10))
      }
    } else {
      content
    }
  }
}

/// A row of the block palette and the command palette: a coloured icon, a title, one line of
/// detail and, on the right, an optional accessory such as a keyboard shortcut.
struct PaletteRow<Accessory: View>: View {
  let symbol: String
  let color: Color
  let title: Text
  let detail: Text
  @ViewBuilder var accessory: Accessory

  var body: some View {
    HStack(spacing: 10) {
      IconTile(symbol: symbol, color: color, size: 28)
      VStack(alignment: .leading, spacing: 1) {
        title
          .lineLimit(1)
        detail
          .font(.caption)
          .foregroundStyle(.secondary)
          .lineLimit(1)
      }
      Spacer(minLength: 8)
      accessory
    }
    .padding(.vertical, 2)
    .accessibilityElement(children: .combine)
  }
}

extension PaletteRow where Accessory == EmptyView {
  init(symbol: String, color: Color, title: Text, detail: Text) {
    self.init(symbol: symbol, color: color, title: title, detail: detail) { EmptyView() }
  }
}
