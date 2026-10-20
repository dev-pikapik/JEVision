import JEVisionCore
import SwiftUI

/// Two versions of a test on one flowchart: added blocks, notes and frames are green, removed
/// red, changed yellow, with what each changed one was and is now listed below.
struct FlowDiffView: View {
  enum Side: Hashable {
    case before, after
  }

  let before: Flow?
  let after: Flow?
  @State private var side = Side.after

  private var diff: FlowDiff { diffFlows(before: before, after: after) }
  private var shown: Side { before == nil ? .after : after == nil ? .before : side }

  var body: some View {
    let diff = diff
    let flow = shown == .before ? before : after
    ScrollView {
      VStack(alignment: .leading, spacing: 14) {
        if before != nil, after != nil {
          Picker("Version", selection: $side) {
            Text("Before").tag(Side.before)
            Text("After").tag(Side.after)
          }
          .pickerStyle(.segmented)
          .labelsHidden()
          .fixedSize()
          .frame(maxWidth: .infinity)
        }
        if let flow {
          DiffCanvas(
            flow: flow, blocks: Self.marks(diff.blocks, on: shown), notes: Self.marks(diff.notes, on: shown),
            sections: Self.marks(diff.sections, on: shown))
        }
        ChangeList(diff: diff, before: before, after: after)
      }
      .padding()
    }
  }

  /// How each item on the shown version is highlighted, by id.
  static func marks(_ changes: [BlockChange], on side: Side) -> [String: DiffMark] {
    var marks: [String: DiffMark] = [:]
    for change in changes {
      switch (change.kind, side) {
      case (.changed, _), (.added, .after), (.removed, .before): marks[change.id] = DiffMark(change.kind)
      default: break
      }
    }
    return marks
  }

  nonisolated static func color(_ kind: BlockChangeKind) -> Color {
    switch kind {
    case .added: .green
    case .removed: .red
    case .changed: .yellow
    }
  }

  nonisolated static func symbol(_ kind: BlockChangeKind) -> String {
    switch kind {
    case .added: "plus.circle.fill"
    case .removed: "minus.circle.fill"
    case .changed: "pencil.circle.fill"
    }
  }

  nonisolated static func title(_ kind: BlockChangeKind) -> String {
    switch kind {
    case .added: String(localized: "Added")
    case .removed: String(localized: "Removed")
    case .changed: String(localized: "Changed")
    }
  }

  /// What a setting is called in the inspector, for a key as it is in the test file.
  static func fieldName(_ key: String) -> String {
    switch key {
    case "type": String(localized: "Block type")
    case "app": String(localized: "App")
    case "files": String(localized: "Files")
    case "newInstance": String(localized: "Open in a new copy of the app")
    case "target": String(localized: "Where to click")
    case "button": String(localized: "Button")
    case "double": String(localized: "Double-click")
    case "waitForChange": String(localized: "Wait until the screen changes")
    case "text": String(localized: "Text")
    case "title": String(localized: "Title")
    case "author": String(localized: "Author")
    case "position": String(localized: "Position")
    case "size": String(localized: "Size")
    case "into": String(localized: "Field")
    case "keys": String(localized: "Keys")
    case "until": String(localized: "Wait Until")
    case "timeout": String(localized: "Time limit")
    case "reference": String(localized: "How the screen is checked")
    case "times": String(localized: "Times")
    case "file": String(localized: "Data File")
    case "test": String(localized: "Test")
    case "name": String(localized: "Name")
    case "description": String(localized: "Description")
    case "testType": String(localized: "Type")
    case "priority": String(localized: "Priority")
    case "status": String(localized: "Status")
    case "tags": String(localized: "Tags")
    case "preconditions": String(localized: "Preconditions")
    case "platform": String(localized: "Platform")
    case "link": String(localized: "Link")
    case "flaky": String(localized: "Flaky")
    case "retries": String(localized: "Retries")
    default: key
    }
  }
}

/// The flowchart with its frames and notes, read-only, scaled down to fit the width.
struct DiffCanvas: View {
  let flow: Flow
  let blocks: [String: DiffMark]
  let notes: [String: DiffMark]
  let sections: [String: DiffMark]
  @State private var width = 0.0

  var body: some View {
    let layout = layoutFlow(flow: flow)
    let scale = layout.width > 0 && width > 0 ? min(1, width / layout.width) : 1
    ZStack(alignment: .topLeading) {
      GridDots(inset: 0)
      ForEach(flow.canvas.sections, id: \.id) { section in
        FrameView(section: section, isPicked: false, isEditing: false, title: .constant(section.title), endEditing: {})
          .modifier(ChangeMark(mark: sections[section.id], radius: 16))
          .accessibilityElement(children: .ignore)
          .accessibilityLabel(Text("Frame"))
          .accessibilityValue(Text(verbatim: [sections[section.id].map(\.title), section.title]
            .compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: ". ")))
          .offset(x: section.rect.x, y: section.rect.y)
      }
      Edges(layout: layout)
      ForEach(layout.edges, id: \.self) { edge in
        if let name = flow.blocks.first(where: { $0.id == edge.from })?.kind.branchName(edge.edge) {
          let at = BranchLabel.position(of: edge)
          BranchLabel(name: name)
            .offset(x: at.x, y: at.y)
        }
      }
      ForEach(layout.blocks, id: \.id) { laid in
        if let block = flow.blocks.first(where: { $0.id == laid.id }) {
          DiffBlockNode(block: block, appName: { AppCatalog.shared.name(of: $0) }, mark: blocks[laid.id])
            .frame(width: laid.rect.width, height: laid.rect.height)
            .offset(x: laid.rect.x, y: laid.rect.y)
        }
      }
      ForEach(flow.canvas.notes, id: \.id) { note in
        NoteView(note: note, isPicked: false, isEditing: false, text: .constant(note.text), endEditing: {})
          .frame(width: note.rect.width, height: note.rect.height)
          .modifier(ChangeMark(mark: notes[note.id], radius: 14))
          .accessibilityElement(children: .ignore)
          .accessibilityLabel(Text("Note"))
          .accessibilityValue(Text(verbatim: [notes[note.id].map(\.title), note.text, note.author]
            .compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: ". ")))
          .offset(x: note.rect.x, y: note.rect.y)
      }
    }
    .frame(width: layout.width, height: layout.height, alignment: .topLeading)
    .scaleEffect(scale, anchor: .topLeading)
    .frame(width: layout.width * scale, height: layout.height * scale, alignment: .topLeading)
    .frame(maxWidth: .infinity)
    .unmirrored()
    .onGeometryChange(for: Double.self) { $0.size.width } action: { width = $0 }
    .accessibilityElement(children: .contain)
  }
}

/// How a block, note or frame stands out on a compared flowchart.
struct DiffMark: Equatable {
  let color: Color
  let symbol: String
  let title: String

  /// Changed on both sides, differently.
  static var conflict: DiffMark {
    DiffMark(color: .orange, symbol: "exclamationmark.triangle.fill", title: String(localized: "Conflict"))
  }
}

extension DiffMark {
  init(_ kind: BlockChangeKind) {
    self.init(color: FlowDiffView.color(kind), symbol: FlowDiffView.symbol(kind), title: FlowDiffView.title(kind))
  }
}

/// The outline and corner badge of a changed note or frame; an unchanged one fades.
private struct ChangeMark: ViewModifier {
  let mark: DiffMark?
  let radius: Double

  func body(content: Content) -> some View {
    content
      .allowsHitTesting(false)
      .overlay {
        if let mark {
          RoundedRectangle(cornerRadius: radius).strokeBorder(mark.color, lineWidth: 2.5)
        }
      }
      .overlay(alignment: .topTrailing) {
        if let mark {
          Image(systemName: mark.symbol)
            .font(.system(size: 16))
            .symbolRenderingMode(.palette)
            .foregroundStyle(.white, mark.color)
            .offset(x: 6, y: -6)
        }
      }
      .opacity(mark == nil ? 0.6 : 1)
  }
}

private struct DiffBlockNode: View {
  let block: Block
  let appName: (String) -> String
  let mark: DiffMark?

  private var type: String { block.kind.typeName }

  var body: some View {
    let summary = BlockSummary.text(block.kind, appName: appName)
    HStack(spacing: 10) {
      IconTile(symbol: BlockInfo.symbol(type), color: BlockInfo.color(ofType: type), size: 34)
      VStack(alignment: .leading, spacing: 1) {
        Text(BlockInfo.title(type))
          .font(.headline)
          .lineLimit(1)
        Text(summary)
          .font(.subheadline)
          .foregroundStyle(.secondary)
          .lineLimit(2)
          .truncationMode(.middle)
      }
      Spacer(minLength: 0)
    }
    .padding(.horizontal, 12)
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .background(.background, in: .rect(cornerRadius: 14))
    .background {
      if let mark { RoundedRectangle(cornerRadius: 14).fill(mark.color.opacity(0.12)) }
    }
    .overlay {
      RoundedRectangle(cornerRadius: 14)
        .strokeBorder(mark.map(\.color) ?? .primary.opacity(0.1), lineWidth: mark == nil ? 1 : 2.5)
    }
    .overlay(alignment: .topTrailing) {
      if let mark {
        Image(systemName: mark.symbol)
          .font(.system(size: 16))
          .symbolRenderingMode(.palette)
          .foregroundStyle(.white, mark.color)
          .offset(x: 6, y: -6)
      }
    }
    .opacity(mark == nil ? 0.6 : 1)
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(Text(BlockInfo.title(type)))
    .accessibilityValue(Text([mark.map(\.title), summary].compactMap { $0 }.joined(separator: ". ")))
  }
}

/// Every difference in words: what was added, removed, and what a changed block was and is.
private struct ChangeList: View {
  let diff: FlowDiff
  let before: Flow?
  let after: Flow?

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      if diff.blocks.isEmpty, diff.testFields.isEmpty, diff.notes.isEmpty, diff.sections.isEmpty {
        (before == nil && after == nil
          ? Text("This test is empty.") : Text("No blocks, notes, frames or test details changed."))
          .foregroundStyle(.secondary)
      }
      if !diff.testFields.isEmpty {
        row(
          symbol: "info.circle.fill", color: .yellow, title: Text("Test details"),
          detail: Text("Changed: \(diff.testFields.map(FlowDiffView.fieldName).formatted(.list(type: .and)))"))
      }
      ForEach(diff.blocks, id: \.id) { change in
        let was = before?.blocks.first { $0.id == change.id }
        let now = after?.blocks.first { $0.id == change.id }
        let block = now ?? was
        if let block {
          row(
            symbol: FlowDiffView.symbol(change.kind), color: FlowDiffView.color(change.kind),
            title: Text("\(FlowDiffView.title(change.kind)): \(BlockInfo.title(block.kind.typeName))"),
            detail: detail(change, was: was.map(summary), now: now.map(summary)))
        }
      }
      ForEach(diff.sections, id: \.id) { change in
        let title = { (section: CanvasSection) in
          section.title.isEmpty ? String(localized: "Untitled Frame") : section.title
        }
        row(
          symbol: FlowDiffView.symbol(change.kind), color: FlowDiffView.color(change.kind),
          title: Text("\(FlowDiffView.title(change.kind)): \(String(localized: "Frame"))"),
          detail: detail(
            change, was: before?.canvas.sections.first { $0.id == change.id }.map(title),
            now: after?.canvas.sections.first { $0.id == change.id }.map(title)))
      }
      ForEach(diff.notes, id: \.id) { change in
        row(
          symbol: FlowDiffView.symbol(change.kind), color: FlowDiffView.color(change.kind),
          title: Text("\(FlowDiffView.title(change.kind)): \(String(localized: "Note"))"),
          detail: detail(
            change, was: before?.canvas.notes.first { $0.id == change.id }?.text,
            now: after?.canvas.notes.first { $0.id == change.id }?.text))
      }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }

  private func summary(_ block: Block) -> String {
    BlockSummary.text(block.kind, appName: { AppCatalog.shared.name(of: $0) })
  }

  /// What an item was and is now, described by `was` and `now`, and what of it changed.
  private func detail(_ change: BlockChange, was: String?, now: String?) -> Text {
    switch change.kind {
    case .added: return Text(verbatim: now ?? "")
    case .removed: return Text(verbatim: was ?? "")
    case .changed:
      let fields = change.fields.map(FlowDiffView.fieldName).formatted(.list(type: .and))
      let old = was ?? "", new = now ?? ""
      guard old != new else { return Text("Changed: \(fields)") }
      return Text("Was: \(old)\nNow: \(new)\nChanged: \(fields)")
    }
  }

  private func row(symbol: String, color: Color, title: Text, detail: Text) -> some View {
    HStack(alignment: .firstTextBaseline, spacing: 8) {
      Image(systemName: symbol)
        .foregroundStyle(color)
        .accessibilityHidden(true)
      VStack(alignment: .leading, spacing: 2) {
        title.font(.headline)
        detail
          .foregroundStyle(.secondary)
          .textSelection(.enabled)
          .fixedSize(horizontal: false, vertical: true)
      }
    }
    .accessibilityElement(children: .combine)
  }
}
