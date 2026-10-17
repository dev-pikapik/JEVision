import JEVisionCore
import SwiftUI

/// Searchable list of block types. ↑↓ move, Return adds, Escape closes.
struct PaletteView: View {
  let pick: (String) -> Void
  let close: () -> Void
  @State private var query = ""
  @State private var highlighted: String?
  @FocusState private var searchFocused: Bool

  private var matches: [BlockSpec] {
    let q = query.trimmingCharacters(in: .whitespaces)
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
                PaletteRow(spec: spec)
                  .tag(spec.typeName)
                  .id(spec.typeName)
                  .contentShape(.rect)
                  .onTapGesture { pick(spec.typeName) }
              }
            }
          }
        }
        .listStyle(.sidebar)
        .scrollContentBackground(.hidden)
        .overlay {
          if matches.isEmpty { ContentUnavailableView.search(text: query) }
        }
        .onChange(of: highlighted) { if let highlighted { proxy.scrollTo(highlighted) } }
      }
    }
    .frame(width: 380, height: 420)
    .glassEffect(.regular, in: .rect(cornerRadius: 20))
    .onAppear {
      searchFocused = true
      highlighted = matches.first?.typeName
    }
    .onChange(of: query) { highlighted = matches.first?.typeName }
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
    let ids = matches.map(\.typeName)
    guard !ids.isEmpty else { return .handled }
    let index = highlighted.flatMap(ids.firstIndex) ?? (step > 0 ? -1 : ids.count)
    highlighted = ids[min(max(index + step, 0), ids.count - 1)]
    return .handled
  }

  private func submit() {
    if let type = highlighted ?? matches.first?.typeName { pick(type) }
  }
}

private struct PaletteRow: View {
  let spec: BlockSpec

  var body: some View {
    HStack(spacing: 10) {
      IconTile(symbol: BlockInfo.symbol(spec.typeName), color: BlockInfo.color(spec.group), size: 28)
      VStack(alignment: .leading, spacing: 1) {
        Text(BlockInfo.title(spec.typeName))
        Text(BlockInfo.explanation(spec.typeName))
          .font(.caption)
          .foregroundStyle(.secondary)
          .lineLimit(2)
      }
    }
    .padding(.vertical, 2)
    .accessibilityElement(children: .combine)
  }
}
