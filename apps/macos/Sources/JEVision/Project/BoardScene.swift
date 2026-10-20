import CoreGraphics
import JEVisionCore

/// What the board in view holds and where, worked out once per change so that drawing and
/// hit-testing look only at the part in view instead of going over every node.
struct BoardScene {
  let board: Board
  let layout: BoardLayout
  /// The core's answer to "what is in this rectangle", for the nodes and arrows of `layout`.
  let index: BoardIndex
  /// Every node of the board, by id, also the ones a folded frame hides.
  let nodes: [String: BoardNode]
  let edges: [String: BoardEdge]
  /// Where each shown node is in `layout.nodes`.
  let places: [String: Int]
  /// For each shown node, where the arrows that leave or enter it are in `layout.edges`.
  let links: [String: [Int]]
  /// The tests inside each frame, by frame id, also the ones a fold hides.
  let frameTests: [String: [String]]
  /// The nodes that are pictures.
  let images: Set<String>

  /// What is in view: positions in `layout.nodes` and `layout.edges`, both in drawing order.
  struct Visible: Equatable {
    var nodes: [Int] = []
    var edges: [Int] = []
  }

  static let empty = BoardScene(Board(nodes: [], edges: [], allTests: false, extra: ""))

  init(_ board: Board) {
    self.init(board, layout: layOutBoard(board: board))
  }

  init(_ board: Board, layout: BoardLayout) {
    self.board = board
    self.layout = layout
    index = BoardIndex(layout: layout)
    nodes = Dictionary(board.nodes.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
    edges = Dictionary(board.edges.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
    places = Dictionary(
      layout.nodes.enumerated().map { ($1.id, $0) }, uniquingKeysWith: { first, _ in first })
    var links: [String: [Int]] = [:]
    for (position, laid) in layout.edges.enumerated() {
      guard let edge = edges[laid.id] else { continue }
      links[edge.from, default: []].append(position)
      if edge.to != edge.from { links[edge.to, default: []].append(position) }
    }
    self.links = links
    frameTests = Dictionary(
      boardFrameTests(board: board).map { ($0.frame, $0.tests) }, uniquingKeysWith: { first, _ in first })
    images = Set(
      board.nodes.compactMap { node in
        if case .file(let file) = node.kind, isBoardImage(file: file) { node.id } else { nil }
      })
  }

  /// The nodes and arrows that touch `area` (board points). `moved` are nodes held somewhere else than
  /// their layout says, like the ones being dragged, `shift` away from it: they count where they are
  /// held, and so do their arrows.
  func visible(in area: CGRect, moved: Set<String> = [], by shift: CGSize = .zero) -> Visible {
    let hits = index.visible(area: Rect(area))
    guard !moved.isEmpty else {
      return Visible(nodes: hits.nodes.map(Int.init), edges: hits.edges.map(Int.init))
    }
    var nodes = Set(hits.nodes.map(Int.init).filter { !moved.contains(layout.nodes[$0].id) })
    var edges = Set(hits.edges.map(Int.init))
    // A node held `shift` away shows in `area` when its place in the layout is in `area` moved back.
    let back = index.visible(area: Rect(area.offsetBy(dx: -shift.width, dy: -shift.height)))
    for position in back.nodes.map(Int.init) where moved.contains(layout.nodes[position].id) {
      nodes.insert(position)
      edges.formUnion(links[layout.nodes[position].id] ?? [])
    }
    return Visible(nodes: nodes.sorted(), edges: edges.sorted())
  }

  /// The topmost node at a board point. A frame counts only by its title bar of height `header`,
  /// so a drag inside it pans, unless `wholeFrames`.
  func hit(_ point: CGPoint, header: Double, wholeFrames: Bool) -> String? {
    let here = Rect(x: point.x, y: point.y, width: 0, height: 0)
    for position in index.visible(area: here).nodes.reversed() {
      let laid = layout.nodes[Int(position)]
      let rect = laid.rect.cgRect
      guard rect.contains(point) else { continue }
      if !wholeFrames, laid.frame, point.y > rect.minY + header { continue }
      return laid.id
    }
    return nil
  }

  /// The shown node to go to from node `id` when moving `heading`: what the arrow keys do.
  func neighbour(of id: String, _ heading: BoardHeading) -> String? {
    guard let place = places[id], let next = index.neighbour(from: UInt32(place), heading: heading) else {
      return nil
    }
    return layout.nodes[Int(next)].id
  }

  /// The shown node nearest to a board point, for the arrow keys when nothing is picked.
  func nearest(to point: CGPoint, in area: CGRect) -> String? {
    visible(in: area).nodes.map { layout.nodes[$0] }.min { lhs, rhs in
      Self.distance(from: point, to: lhs.rect.cgRect) < Self.distance(from: point, to: rhs.rect.cgRect)
    }?.id
  }

  private static func distance(from point: CGPoint, to rect: CGRect) -> Double {
    hypot(point.x - rect.midX, point.y - rect.midY)
  }
}
