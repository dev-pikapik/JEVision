import SwiftUI

/// A test drawn the way the Mac editor lays it out, read-only, scaled to fit the width.
struct FlowView: View {
  let flow: TestFlow
  var marks: [String: StepStatus] = [:]
  var current: String?
  var maxHeight: CGFloat = .infinity
  @State private var width: CGFloat = 0

  var body: some View {
    let scale = width > 0 ? min(1, max(0.45, width / max(flow.size.width, 1))) : 1
    ScrollView([.horizontal, .vertical]) {
      canvas
        .scaleEffect(scale, anchor: .topLeading)
        .frame(width: flow.size.width * scale, height: flow.size.height * scale, alignment: .topLeading)
        .frame(minWidth: width)
    }
    .scrollBounceBehavior(.basedOnSize)
    .frame(height: min(flow.size.height * scale, maxHeight))
    .frame(maxWidth: .infinity)
    .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
  }

  private var canvas: some View {
    ZStack(alignment: .topLeading) {
      ForEach(flow.frames, id: \.self) { frame in
        RoundedRectangle(cornerRadius: 16)
          .fill(BlockInfo.color(kind(of: frame.block)).opacity(0.05))
          .strokeBorder(.separator, style: StrokeStyle(lineWidth: 1, dash: [5, 4]))
          .frame(width: frame.area.width, height: frame.area.height)
          .offset(x: frame.area.minX, y: frame.area.minY)
      }
      Edges(flow: flow)
      ForEach(flow.edges.filter { ($0.edge == "then" || $0.edge == "else") && !$0.points.isEmpty }, id: \.self) { edge in
        let point = edge.points[min(2, edge.points.count - 1)]
        Text(edge.edge == "then" ? "Yes" : "No")
          .font(.caption2.weight(.semibold))
          .foregroundStyle(.secondary)
          .offset(x: point.x + 6, y: point.y + 3)
      }
      ForEach(Array(steps.enumerated()), id: \.element.id) { index, block in
        if let area = block.area {
          BlockCard(block: block, mark: marks[block.id], isCurrent: current == block.id, step: index + 1, steps: steps.count)
            .frame(width: area.width, height: area.height)
            .offset(x: area.minX, y: area.minY)
        }
      }
    }
    .frame(width: flow.size.width, height: flow.size.height, alignment: .topLeading)
  }

  private var steps: [TestFlow.Block] { flow.blocks.filter { $0.area != nil } }

  private func kind(of block: String) -> String {
    flow.blocks.first { $0.id == block }?.kind ?? ""
  }
}

private struct Edges: View {
  let flow: TestFlow

  var body: some View {
    Canvas { context, _ in
      for edge in flow.edges where edge.points.count > 1 {
        let points = edge.points
        context.stroke(Self.line(points), with: .style(.secondary), style: StrokeStyle(lineWidth: 1.5, lineJoin: .round))
        context.fill(Self.head(from: points[points.count - 2], to: points[points.count - 1]), with: .style(.secondary))
      }
    }
    .frame(width: flow.size.width, height: flow.size.height)
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

private struct BlockCard: View {
  let block: TestFlow.Block
  let mark: StepStatus?
  let isCurrent: Bool
  let step: Int
  let steps: Int

  private var border: (color: Color, width: Double) {
    if isCurrent { return (.orange, 2.5) }
    if mark == .failed { return (.red, 2) }
    return (.primary.opacity(0.1), 1)
  }

  var body: some View {
    HStack(spacing: 10) {
      IconTile(symbol: BlockInfo.symbol(block.kind), color: BlockInfo.color(block.kind), size: 34)
      VStack(alignment: .leading, spacing: 1) {
        Text(BlockInfo.title(block.kind))
          .font(.headline)
          .lineLimit(1)
          .minimumScaleFactor(0.6)
        if !block.title.isEmpty {
          Text(block.title)
            .font(.subheadline)
            .foregroundStyle(.secondary)
            .lineLimit(2)
            .truncationMode(.middle)
        }
      }
      Spacer(minLength: 0)
    }
    .padding(.horizontal, 12)
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .background(.background, in: .rect(cornerRadius: 14))
    .overlay {
      RoundedRectangle(cornerRadius: 14)
        .strokeBorder(border.color, lineWidth: border.width)
    }
    .overlay(alignment: .topTrailing) {
      if let mark {
        VerdictStyle(mark).image
          .font(.system(size: 18))
          .background(Circle().fill(.background).padding(2))
          .offset(x: 7, y: -7)
      }
    }
    .shadow(color: isCurrent ? .orange.opacity(0.45) : .black.opacity(0.08), radius: isCurrent ? 10 : 3, y: 1)
    .animation(.snappy, value: isCurrent)
    .animation(.snappy, value: mark)
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(Text("Step \(step) of \(steps), \(BlockInfo.title(block.kind)), \(block.title)"))
    .accessibilityValue(Text(value))
  }

  private var value: String {
    if isCurrent { return String(localized: "Running now") }
    return mark.map { VerdictStyle($0).title } ?? ""
  }
}

#Preview {
  FlowView(
    flow: TestFlow(
      name: "Sign in", description: nil, app: "com.example.Notes", start: "a",
      size: CGSize(width: 320, height: 330),
      blocks: [
        .init(id: "a", kind: "openApp", title: "Notes", area: CGRect(x: 20, y: 20, width: 280, height: 60)),
        .init(id: "b", kind: "click", title: "New Note", area: CGRect(x: 20, y: 130, width: 280, height: 60)),
        .init(id: "c", kind: "check", title: "empty-note.png", area: CGRect(x: 20, y: 240, width: 280, height: 60)),
      ],
      frames: [],
      edges: [
        .init(from: "a", edge: "next", to: "b", points: [CGPoint(x: 160, y: 80), CGPoint(x: 160, y: 130)]),
        .init(from: "b", edge: "next", to: "c", points: [CGPoint(x: 160, y: 190), CGPoint(x: 160, y: 240)]),
      ]),
    marks: ["a": .passed], current: "b")
}
