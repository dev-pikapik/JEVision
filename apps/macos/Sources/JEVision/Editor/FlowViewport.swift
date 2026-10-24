import JEVisionCore
import SwiftUI

extension View {
  /// Lay out text, symbols and borders at their displayed size, then return to world units.
  /// The camera's transform cancels this inverse transform, preserving native glyph rendering.
  func canvasElementSize(_ size: CGSize, scale: Double) -> some View {
    frame(width: size.width * scale, height: size.height * scale)
      .scaleEffect(1 / scale, anchor: .topLeading)
      .frame(width: size.width, height: size.height, alignment: .topLeading)
  }
}

/// A camera on an unbounded test canvas. Resizing a panel never changes its origin.
struct FlowViewport: Equatable {
  var offset = CGPoint.zero
  var scale = 1.0

  func world(_ point: CGPoint) -> CGPoint {
    CGPoint(x: (point.x - offset.x) / scale, y: (point.y - offset.y) / scale)
  }

  func view(_ point: CGPoint) -> CGPoint {
    CGPoint(x: point.x * scale + offset.x, y: point.y * scale + offset.y)
  }

  func visible(in size: CGSize) -> CGRect {
    CGRect(
      origin: world(.zero), size: CGSize(width: size.width / scale, height: size.height / scale))
  }

  mutating func zoom(to scale: Double, around anchor: CGPoint) {
    let point = world(anchor)
    self.scale = Zoom.clamp(scale)
    offset = CGPoint(x: anchor.x - point.x * self.scale, y: anchor.y - point.y * self.scale)
  }

  mutating func center(on point: CGPoint, in size: CGSize) {
    offset = CGPoint(x: size.width / 2 - point.x * scale, y: size.height / 2 - point.y * scale)
  }

  static func bounds(layout: FlowLayout, canvas: CanvasContent?, padding: Double = 32) -> CGRect {
    let rects =
      layout.blocks.map(\.rect.cgRect) + layout.frames.map(\.rect.cgRect)
      + (canvas?.sections.map(\.rect.cgRect) ?? []) + (canvas?.notes.map(\.rect.cgRect) ?? [])
    var bounds = rects.first ?? CGRect(x: 0, y: 0, width: 208, height: 64)
    for rect in rects.dropFirst() { bounds = bounds.union(rect) }
    for point in layout.edges.flatMap(\.points) + layout.slots.map(\.at) {
      bounds = bounds.union(CGRect(x: point.x, y: point.y, width: 1, height: 1))
    }
    return bounds.insetBy(dx: -padding, dy: -padding)
  }
}

/// Reversible world-to-minimap projection, including negative coordinates and the viewport.
struct FlowMapProjection {
  let world: CGRect
  let scale: Double
  let offset: CGPoint

  init(content: CGRect, visible: CGRect, size: CGSize) {
    let union = content.union(visible)
    world = union.insetBy(dx: -max(32, union.width * 0.05), dy: -max(32, union.height * 0.05))
    scale = min(size.width / world.width, size.height / world.height)
    offset = CGPoint(
      x: (size.width - world.width * scale) / 2 - world.minX * scale,
      y: (size.height - world.height * scale) / 2 - world.minY * scale)
  }

  func point(_ point: CGPoint) -> CGPoint {
    CGPoint(x: point.x * scale + offset.x, y: point.y * scale + offset.y)
  }

  func worldPoint(_ point: CGPoint) -> CGPoint {
    CGPoint(x: (point.x - offset.x) / scale, y: (point.y - offset.y) / scale)
  }

  func rect(_ rect: CGRect) -> CGRect {
    CGRect(
      origin: point(rect.origin),
      size: CGSize(width: rect.width * scale, height: rect.height * scale))
  }
}

struct FlowMinimap: View {
  let layout: FlowLayout
  let canvas: CanvasContent?
  let visible: CGRect
  let picked: Set<CanvasItem>
  let jump: (CGPoint) -> Void
  static let size = CGSize(width: 200, height: 132)
  @State private var dragProjection: FlowMapProjection?

  var body: some View {
    let projection =
      dragProjection
      ?? FlowMapProjection(
        content: FlowViewport.bounds(layout: layout, canvas: canvas), visible: visible,
        size: Self.size)
    Canvas { context, _ in
      for section in canvas?.sections ?? [] {
        context.fill(
          Path(projection.rect(section.rect.cgRect)), with: .color(.secondary.opacity(0.15)))
      }
      for edge in layout.edges where edge.points.count > 1 {
        var path = Path()
        path.addLines(edge.points.map { projection.point(CGPoint(x: $0.x, y: $0.y)) })
        context.stroke(path, with: .color(.secondary.opacity(0.6)), lineWidth: 1)
      }
      for block in layout.blocks {
        let rect = projection.rect(block.rect.cgRect)
        context.fill(
          Path(roundedRect: rect, cornerRadius: 2),
          with: .color(
            picked.contains(.block(id: block.id)) ? .accentColor : .secondary.opacity(0.7)))
      }
      for note in canvas?.notes ?? [] {
        context.fill(Path(projection.rect(note.rect.cgRect)), with: .color(.yellow.opacity(0.65)))
      }
      let view = Path(roundedRect: projection.rect(visible), cornerRadius: 3)
      context.fill(view, with: .color(.accentColor.opacity(0.08)))
      context.stroke(view, with: .color(.accentColor), lineWidth: 1.5)
    }
    .frame(width: Self.size.width, height: Self.size.height)
    .contentShape(.rect)
    .gesture(
      DragGesture(minimumDistance: 0)
        .onChanged { value in
          if dragProjection == nil { dragProjection = projection }
          jump(projection.worldPoint(value.location))
        }
        .onEnded { _ in dragProjection = nil }
    )
    .accessibilityElement()
    .accessibilityLabel(Text("Minimap"))
    .accessibilityValue(Text("\(layout.blocks.count) blocks"))
    .accessibilityAdjustableAction { direction in
      let delta = direction == .increment ? visible.width / 2 : -visible.width / 2
      jump(CGPoint(x: visible.midX + delta, y: visible.midY))
    }
  }
}
