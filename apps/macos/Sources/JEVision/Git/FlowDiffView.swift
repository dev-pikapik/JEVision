import JEVisionCore
import SwiftUI

/// Two versions of a test on one flowchart: added blocks are green, removed red, changed
/// yellow, with what each changed block was and is now listed below.
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
          DiffCanvas(flow: flow, marks: marks(diff, on: shown))
        }
        ChangeList(diff: diff, before: before, after: after)
      }
      .padding()
    }
  }

  /// How each block on the shown version is highlighted.
  private func marks(_ diff: FlowDiff, on side: Side) -> [String: BlockChangeKind] {
    var marks: [String: BlockChangeKind] = [:]
    for change in diff.blocks {
      switch (change.kind, side) {
      case (.changed, _), (.added, .after), (.removed, .before): marks[change.id] = change.kind
      default: break
      }
    }
    return marks
  }

  static func color(_ kind: BlockChangeKind) -> Color {
    switch kind {
    case .added: .green
    case .removed: .red
    case .changed: .yellow
    }
  }

  static func symbol(_ kind: BlockChangeKind) -> String {
    switch kind {
    case .added: "plus.circle.fill"
    case .removed: "minus.circle.fill"
    case .changed: "pencil.circle.fill"
    }
  }

  static func title(_ kind: BlockChangeKind) -> String {
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
    case "retries": String(localized: "Retries")
    default: key
    }
  }
}

/// The flowchart, read-only, scaled down to fit the width.
private struct DiffCanvas: View {
  let flow: Flow
  let marks: [String: BlockChangeKind]
  @State private var width = 0.0

  var body: some View {
    let layout = layoutFlow(flow: flow)
    let scale = layout.width > 0 && width > 0 ? min(1, width / layout.width) : 1
    ZStack(alignment: .topLeading) {
      Edges(layout: layout)
      ForEach(layout.edges.filter { $0.edge == "then" || $0.edge == "else" }, id: \.self) { edge in
        BranchLabel(isThen: edge.edge == "then")
          .offset(x: edge.points[min(2, edge.points.count - 1)].x + 6, y: edge.points[min(2, edge.points.count - 1)].y + 3)
      }
      ForEach(layout.blocks, id: \.id) { laid in
        if let block = flow.blocks.first(where: { $0.id == laid.id }) {
          DiffBlockNode(block: block, appName: { AppCatalog.shared.name(of: $0) }, mark: marks[laid.id])
            .frame(width: laid.rect.width, height: laid.rect.height)
            .offset(x: laid.rect.x, y: laid.rect.y)
        }
      }
    }
    .frame(width: layout.width, height: layout.height, alignment: .topLeading)
    .scaleEffect(scale, anchor: .topLeading)
    .frame(width: layout.width * scale, height: layout.height * scale, alignment: .topLeading)
    .frame(maxWidth: .infinity)
    .onGeometryChange(for: Double.self) { $0.size.width } action: { width = $0 }
    .accessibilityElement(children: .contain)
  }
}

private struct DiffBlockNode: View {
  let block: Block
  let appName: (String) -> String
  let mark: BlockChangeKind?

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
      if let mark { RoundedRectangle(cornerRadius: 14).fill(FlowDiffView.color(mark).opacity(0.12)) }
    }
    .overlay {
      RoundedRectangle(cornerRadius: 14)
        .strokeBorder(mark.map(FlowDiffView.color) ?? .primary.opacity(0.1), lineWidth: mark == nil ? 1 : 2.5)
    }
    .overlay(alignment: .topTrailing) {
      if let mark {
        Image(systemName: FlowDiffView.symbol(mark))
          .font(.system(size: 16))
          .symbolRenderingMode(.palette)
          .foregroundStyle(.white, FlowDiffView.color(mark))
          .offset(x: 6, y: -6)
      }
    }
    .opacity(mark == nil ? 0.6 : 1)
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(Text(BlockInfo.title(type)))
    .accessibilityValue(Text([mark.map(FlowDiffView.title), summary].compactMap { $0 }.joined(separator: ". ")))
  }
}

/// Every difference in words: what was added, removed, and what a changed block was and is.
private struct ChangeList: View {
  let diff: FlowDiff
  let before: Flow?
  let after: Flow?

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      if diff.blocks.isEmpty, diff.testFields.isEmpty {
        (before == nil && after == nil ? Text("This test is empty.") : Text("No blocks or test details changed."))
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
            detail: detail(change, was: was, now: now))
        }
      }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }

  private func detail(_ change: BlockChange, was: Block?, now: Block?) -> Text {
    let appName = { AppCatalog.shared.name(of: $0) }
    switch change.kind {
    case .added: return Text(now.map { BlockSummary.text($0.kind, appName: appName) } ?? "")
    case .removed: return Text(was.map { BlockSummary.text($0.kind, appName: appName) } ?? "")
    case .changed:
      let fields = change.fields.map(FlowDiffView.fieldName).formatted(.list(type: .and))
      let old = was.map { BlockSummary.text($0.kind, appName: appName) } ?? ""
      let new = now.map { BlockSummary.text($0.kind, appName: appName) } ?? ""
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
