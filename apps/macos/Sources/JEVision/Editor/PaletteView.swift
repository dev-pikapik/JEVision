import JEVisionCore
import SwiftUI

/// What the palette adds.
enum PaletteChoice: Hashable {
  case block(String)
  case note
  case frame

  var title: String {
    switch self {
    case .block(let type): BlockInfo.title(type)
    case .note: String(localized: "Note")
    case .frame: String(localized: "Frame")
    }
  }

  var explanation: String {
    switch self {
    case .block(let type): BlockInfo.explanation(type)
    case .note: String(localized: "A sticky note for whoever reads the test")
    case .frame: String(localized: "Groups blocks into a section with a title")
    }
  }
}

/// Searchable list of block types, then a note and a frame. ↑↓ move, Return adds, Escape closes.
struct PaletteView: View {
  static let size = CGSize(width: 380, height: 420)
  let pick: (PaletteChoice) -> Void
  let close: () -> Void
  @State private var query = ""
  @State private var highlighted: PaletteChoice?
  @FocusState private var searchFocused: Bool

  private var trimmedQuery: String { query.trimmingCharacters(in: .whitespaces) }

  /// The note and the frame, when they match the search.
  private var extras: [PaletteChoice] {
    [PaletteChoice.note, .frame].filter {
      trimmedQuery.isEmpty || $0.title.localizedStandardContains(trimmedQuery)
        || $0.explanation.localizedStandardContains(trimmedQuery)
    }
  }

  private var choices: [PaletteChoice] {
    matches.map { .block($0.typeName) } + extras
  }

  private var matches: [BlockSpec] {
    let q = trimmedQuery
    guard !q.isEmpty else { return BlockInfo.catalog }
    return BlockInfo.catalog.filter {
      BlockInfo.title($0.typeName).localizedStandardContains(q)
        || BlockInfo.explanation($0.typeName).localizedStandardContains(q)
        || BlockInfo.groupTitle($0.group).localizedStandardContains(q)
    }
  }

  private var groups: [(BlockGroup, [BlockSpec])] {
    var result: [(BlockGroup, [BlockSpec])] = []
    for spec in matches {
      if result.last?.0 == spec.group { result[result.count - 1].1.append(spec) } else { result.append((spec.group, [spec])) }
    }
    return result
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
          ForEach(groups, id: \.0) { group, specs in
            Section(BlockInfo.groupTitle(group)) {
              ForEach(specs, id: \.typeName) { spec in
                PaletteRow(
                  symbol: BlockInfo.symbol(spec.typeName), color: BlockInfo.color(spec.group),
                  title: Text(BlockInfo.title(spec.typeName)), explanation: Text(BlockInfo.explanation(spec.typeName))
                )
                .tag(PaletteChoice.block(spec.typeName))
                .id(PaletteChoice.block(spec.typeName))
                .contentShape(.rect)
                .onTapGesture { pick(.block(spec.typeName)) }
              }
            }
          }
          if !extras.isEmpty {
            Section("Canvas") {
              ForEach(extras, id: \.self) { choice in
                PaletteRow(
                  symbol: choice == .note ? "note.text" : "rectangle.dashed", color: choice == .note ? .yellow : .gray,
                  title: Text(choice.title), explanation: Text(choice.explanation)
                )
                .tag(choice)
                .id(choice)
                .contentShape(.rect)
                .onTapGesture { pick(choice) }
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
    }
    .frame(width: Self.size.width, height: Self.size.height)
    .glassEffect(.regular, in: .rect(cornerRadius: 20))
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

private struct PaletteRow: View {
  let symbol: String
  let color: Color
  let title: Text
  let explanation: Text

  var body: some View {
    HStack(spacing: 10) {
      IconTile(symbol: symbol, color: color, size: 28)
      VStack(alignment: .leading, spacing: 1) {
        title
        explanation
          .font(.caption)
          .foregroundStyle(.secondary)
          .lineLimit(2)
      }
    }
    .padding(.vertical, 2)
    .accessibilityElement(children: .combine)
  }
}
