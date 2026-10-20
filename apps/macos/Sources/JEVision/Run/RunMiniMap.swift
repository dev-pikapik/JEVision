import JEVisionCore
import SwiftUI

/// The test's blocks as a small map for the run panel: where each one sits, which are done, which
/// failed and which one runs now. A status shows by its shape as well as its colour, the same ones
/// the sidebar uses for a finished run.
struct RunMiniMap: Equatable {
  enum Status: Equatable {
    case pending, passed, failed, current(paused: Bool)

    var symbol: String {
      switch self {
      case .pending: "circle"
      case .passed: RunStatus.passed.dot
      case .failed: RunStatus.failed.dot
      case .current(let paused): paused ? "pause.circle.fill" : "play.circle.fill"
      }
    }

    var color: Color {
      switch self {
      case .pending: .secondary
      case .passed: RunStatus.passed.color
      case .failed: RunStatus.failed.color
      case .current(let paused): paused ? .orange : .accentColor
      }
    }

    var size: Double {
      if case .current = self { 14 } else { 8 }
    }
  }

  struct Node: Equatable {
    let id: String
    let at: CGPoint
    let status: Status
  }

  /// A connector; `reached` once the run got to the block it leads to.
  struct Link: Equatable {
    let points: [CGPoint]
    let reached: Bool
  }

  /// The smallest scale at which blocks stay apart; a longer test shows the part around the
  /// block that runs now.
  static let smallest = 0.1

  let nodes: [Node]
  let links: [Link]

  /// Lays `layout` out in a box of `size`, marked as `state` says.
  init(layout: FlowLayout, state: RunState, in size: CGSize) {
    guard layout.width > 0, layout.height > 0 else {
      nodes = []
      links = []
      return
    }
    let scale = max(min(Double(size.width) / layout.width, Double(size.height) / layout.height), Self.smallest)
    let focus = layout.blocks.first { $0.id == state.current }.map {
      CGPoint(x: ($0.rect.x + $0.rect.width / 2) * scale, y: ($0.rect.y + $0.rect.height / 2) * scale)
    }
    let origin = CGPoint(
      x: Self.offset(content: layout.width * scale, box: Double(size.width), focus: Double(focus?.x ?? 0)),
      y: Self.offset(content: layout.height * scale, box: Double(size.height), focus: Double(focus?.y ?? 0)))
    let place = { (x: Double, y: Double) in CGPoint(x: origin.x + x * scale, y: origin.y + y * scale) }
    let status = layout.blocks.reduce(into: [String: Status]()) { $0[$1.id] = Self.status(of: $1.id, in: state) }
    nodes = layout.blocks.map {
      Node(id: $0.id, at: place($0.rect.x + $0.rect.width / 2, $0.rect.y + $0.rect.height / 2), status: status[$0.id] ?? .pending)
    }
    links = layout.edges.map { edge in
      Link(points: edge.points.map { place($0.x, $0.y) }, reached: status[edge.to].map { $0 != .pending } ?? false)
    }
  }

  /// Where a content `content` long starts in a box `box` long: in the middle when it fits, else
  /// moved so that `focus` is in the middle as far as the content reaches.
  private static func offset(content: Double, box: Double, focus: Double) -> Double {
    content <= box ? (box - content) / 2 : min(0, max(box - content, box / 2 - focus))
  }

  static func status(of block: String, in state: RunState) -> Status {
    if state.current == block { return .current(paused: state.isPaused) }
    switch state.marks[block] {
    case .passed: return .passed
    case .failed: return .failed
    default: return .pending
    }
  }

  /// What the map says, in words, for VoiceOver.
  static func summary(of layout: FlowLayout, state: RunState) -> String {
    let statuses = layout.blocks.map { status(of: $0.id, in: state) }
    let passed = statuses.filter { $0 == .passed }.count
    let failed = statuses.filter { $0 == .failed }.count
    return String(localized: "\(passed) passed, \(failed) failed, \(statuses.count - passed - failed) to go")
  }
}

struct RunMiniMapView: View {
  static let height = 84.0
  let layout: FlowLayout
  let state: RunState

  var body: some View {
    GeometryReader { proxy in
      let map = RunMiniMap(layout: layout, state: state, in: proxy.size)
      ZStack {
        Canvas { context, _ in
          for link in map.links where link.points.count > 1 {
            var path = Path()
            path.addLines(link.points)
            context.stroke(path, with: .color(.secondary.opacity(link.reached ? 0.7 : 0.3)), lineWidth: 1)
          }
        }
        ForEach(map.nodes, id: \.id) { node in
          Image(systemName: node.status.symbol)
            .font(.system(size: node.status.size))
            .foregroundStyle(node.status.color)
            .position(node.at)
        }
      }
    }
    .frame(height: Self.height)
    .padding(8)
    .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: 10))
    .clipShape(.rect(cornerRadius: 10))
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(Text("Map of the test"))
    .accessibilityValue(Text(RunMiniMap.summary(of: layout, state: state)))
  }
}
