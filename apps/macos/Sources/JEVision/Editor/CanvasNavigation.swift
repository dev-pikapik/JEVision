import Foundation
import JEVisionCore

/// The way an arrow key moves on the canvas.
enum CanvasDirection {
  case up, down, left, right
}

/// Which block an arrow key goes to on the canvas.
enum CanvasNavigation {
  /// The edges down follows, in the order they are tried: into a loop first, then the Yes branch.
  private static let downward = ["body", "then", "next", "else"]

  /// The block an arrow key goes to from block `id`. Down follows its link to the next step, up
  /// goes back to the step that leads to it, left and right go to another branch of the same
  /// step; where there is no such link, to the nearest block that way. Without a block, down and
  /// right go to the first block in reading order `order`, up and left to the last.
  static func next(from id: String?, toward direction: CanvasDirection, in layout: FlowLayout, order: [String])
    -> String?
  {
    guard let id, let from = layout.blocks.first(where: { $0.id == id }) else {
      return direction == .down || direction == .right ? order.first : order.last
    }
    let edges = layout.edges.filter { $0.from != $0.to }
    let linked: String?
    switch direction {
    case .down:
      linked = edges.filter { $0.from == id }.min { rank($0.edge) < rank($1.edge) }?.to
    case .up:
      // Of several steps leading here, the one read last before this block: the one just above.
      let at = { order.firstIndex(of: $0) ?? order.count }
      let sources = edges.filter { $0.to == id }.map(\.from).sorted { at($0) < at($1) }
      linked = sources.last { at($0) < at(id) } ?? sources.first
    case .left, .right:
      let parents = Set(edges.filter { $0.to == id }.map(\.from))
      let siblings = Set(edges.filter { parents.contains($0.from) && $0.to != id }.map(\.to))
      linked = nearest(from: from.rect, among: layout.blocks.filter { siblings.contains($0.id) }, toward: direction)
    }
    return linked ?? nearest(from: from.rect, among: layout.blocks, toward: direction)
  }

  /// The block of `blocks` closest to `rect` in `direction`, preferring ones in line with it.
  static func nearest(from rect: Rect, among blocks: [LaidBlock], toward direction: CanvasDirection) -> String? {
    let a = rect.cgRect
    return blocks
      .compactMap { block -> (id: String, cost: Double)? in
        let b = block.rect.cgRect
        let dx = b.midX - a.midX, dy = b.midY - a.midY
        let (ahead, across) =
          switch direction {
          case .up: (-dy, dx)
          case .down: (dy, dx)
          case .left: (-dx, dy)
          case .right: (dx, dy)
          }
        return ahead > 0.5 ? (block.id, ahead + abs(across) * 2) : nil
      }
      .min { $0.cost < $1.cost }?.id
  }

  /// How far apart the centers of `a` and `b` are.
  static func distance(_ a: Rect, _ b: Rect) -> Double {
    hypot(a.x + a.width / 2 - b.x - b.width / 2, a.y + a.height / 2 - b.y - b.height / 2)
  }

  private static func rank(_ edge: String) -> Int {
    downward.firstIndex(of: edge) ?? downward.count
  }
}
