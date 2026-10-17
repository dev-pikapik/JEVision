import JEVisionCore
import SwiftUI

/// The test drawn as a flowchart, at the positions the core laid out. Unscaled: the editor zooms it.
struct FlowCanvas: View {
  let document: FlowDocument
  let appName: (String) -> String
  let onSelect: (String) -> Void
  let onAdd: (Slot) -> Void

  var body: some View {
    let layout = document.layout
    let order = document.order
    ZStack(alignment: .topLeading) {
      Edges(layout: layout)
      ForEach(layout.frames, id: \.block) { frame in
        RoundedRectangle(cornerRadius: 16)
          .fill(BlockInfo.color(ofType: document.block(frame.block)?.kind.typeName ?? "").opacity(0.05))
          .strokeBorder(.separator, style: StrokeStyle(lineWidth: 1, dash: [5, 4]))
          .frame(width: frame.rect.width, height: frame.rect.height)
          .offset(x: frame.rect.x, y: frame.rect.y)
          .accessibilityHidden(true)
      }
      ForEach(layout.edges.filter { $0.edge == "then" || $0.edge == "else" }, id: \.self) { edge in
        BranchLabel(isThen: edge.edge == "then")
          .offset(x: edge.points[min(2, edge.points.count - 1)].x + 6, y: edge.points[min(2, edge.points.count - 1)].y + 3)
      }
      ForEach(layout.blocks, id: \.id) { laid in
        if let block = document.block(laid.id) {
          BlockNode(
            block: block,
            summary: BlockSummary.text(block.kind, appName: appName),
            issues: document.issues(of: laid.id),
            isSelected: document.selection == laid.id,
            step: (order.firstIndex(of: laid.id) ?? 0) + 1,
            steps: order.count
          ) { onSelect(laid.id) }
          .frame(width: laid.rect.width, height: laid.rect.height)
          .offset(x: laid.rect.x, y: laid.rect.y)
        }
      }
      ForEach(layout.slots, id: \.slot) { laid in
        SlotButton(
          isPinned: layout.blocks.isEmpty || (document.selection != nil && laid.slot.from == document.selection),
          label: slotLabel(laid.slot)
        ) { onAdd(laid.slot) }
        .offset(x: laid.at.x - SlotButton.size / 2, y: laid.at.y - SlotButton.size / 2)
      }
    }
    .frame(width: layout.width, height: layout.height, alignment: .topLeading)
  }

  private func slotLabel(_ slot: Slot) -> Text {
    guard let block = document.block(slot.from) else { return Text("Add a block at the start") }
    let title = BlockInfo.title(block.kind.typeName)
    switch slot.edge {
    case "then": return Text("Add a block to Yes after \(title)")
    case "else": return Text("Add a block to No after \(title)")
    case "body": return Text("Add a block inside \(title)")
    default: return Text("Add a block after \(title)")
    }
  }
}

/// Arrows between blocks: lines with rounded corners and a head at the end, over a dot grid.
private struct Edges: View {
  let layout: FlowLayout

  var body: some View {
    Canvas { context, size in
      var dots = Path()
      for x in stride(from: 10.0, to: size.width, by: 20) {
        for y in stride(from: 10.0, to: size.height, by: 20) {
          dots.addEllipse(in: CGRect(x: x - 0.75, y: y - 0.75, width: 1.5, height: 1.5))
        }
      }
      context.fill(dots, with: .style(.quaternary))
      for edge in layout.edges where edge.points.count > 1 {
        let points = edge.points.map { CGPoint(x: $0.x, y: $0.y) }
        context.stroke(Self.line(points), with: .style(.secondary), style: StrokeStyle(lineWidth: 1.5, lineJoin: .round))
        context.fill(Self.head(from: points[points.count - 2], to: points[points.count - 1]), with: .style(.secondary))
      }
    }
    .frame(width: layout.width, height: layout.height)
    .accessibilityHidden(true)
  }

  static func line(_ points: [CGPoint]) -> Path {
    var path = Path()
    path.move(to: points[0])
    for i in 1..<points.count - 1 {
      let a = points[i - 1], b = points[i], c = points[i + 1]
      let room = min(hypot(b.x - a.x, b.y - a.y), hypot(c.x - b.x, c.y - b.y)) / 2
      path.addArc(tangent1End: b, tangent2End: c, radius: min(8, room))
    }
    path.addLine(to: points[points.count - 1])
    return path
  }

  static func head(from a: CGPoint, to b: CGPoint) -> Path {
    let angle = atan2(b.y - a.y, b.x - a.x)
    let length = 8.0, spread = 0.45
    var path = Path()
    path.move(to: b)
    path.addLine(to: CGPoint(x: b.x - length * cos(angle - spread), y: b.y - length * sin(angle - spread)))
    path.addLine(to: CGPoint(x: b.x - length * cos(angle + spread), y: b.y - length * sin(angle + spread)))
    path.closeSubpath()
    return path
  }
}

private struct BranchLabel: View {
  let isThen: Bool

  var body: some View {
    Text(isThen ? "Yes" : "No")
      .font(.caption2.weight(.semibold))
      .foregroundStyle(.secondary)
      .accessibilityHidden(true)
  }
}

private struct BlockNode: View {
  let block: Block
  let summary: String
  let issues: [Issue]
  let isSelected: Bool
  let step: Int
  let steps: Int
  let select: () -> Void

  private var type: String { block.kind.typeName }
  private var hasError: Bool { issues.contains { $0.severity == .error } }

  var body: some View {
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
      if !issues.isEmpty {
        Image(systemName: hasError ? "exclamationmark.octagon.fill" : "exclamationmark.triangle.fill")
          .foregroundStyle(hasError ? .red : .orange)
          .help(issues.map(IssueText.line).joined(separator: "\n"))
      }
    }
    .padding(.horizontal, 12)
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .background(.background, in: .rect(cornerRadius: 14))
    .overlay {
      RoundedRectangle(cornerRadius: 14)
        .strokeBorder(isSelected ? Color.accentColor : Color.primary.opacity(0.1), lineWidth: isSelected ? 2.5 : 1)
    }
    .shadow(color: .black.opacity(0.08), radius: 3, y: 1)
    .contentShape(.rect(cornerRadius: 14))
    .onTapGesture(perform: select)
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(Text("Step \(step) of \(steps), \(BlockInfo.title(type))"))
    .accessibilityValue(Text(([summary] + issues.map(IssueText.line)).joined(separator: ". ")))
    .accessibilityAddTraits(isSelected ? [.isButton, .isSelected] : .isButton)
    .accessibilityAction(.default, select)
  }
}

/// The “+” on an arrow that adds a block there. Shown on hover and next to the selected block.
private struct SlotButton: View {
  static let size = 22.0
  let isPinned: Bool
  let label: Text
  let add: () -> Void
  @State private var isHovered = false

  var body: some View {
    Button(action: add) {
      Image(systemName: "plus")
        .font(.system(size: 11, weight: .bold))
        .frame(width: Self.size, height: Self.size)
        .background(.background, in: .circle)
        .overlay { Circle().strokeBorder(Color.accentColor, lineWidth: 1.5) }
        .foregroundStyle(Color.accentColor)
    }
    .buttonStyle(.plain)
    .opacity(isPinned || isHovered ? 1 : 0)
    .padding(10)
    .contentShape(.rect)
    .onHover { isHovered = $0 }
    .padding(-10)
    .help(label)
    .accessibilityLabel(label)
  }
}
