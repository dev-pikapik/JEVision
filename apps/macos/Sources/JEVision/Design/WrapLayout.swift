import SwiftUI

/// Lays its views out in rows, starting a new row when the next one doesn't fit. Buttons and
/// chips go here instead of an `HStack`, so a longer translation or a larger font wraps them
/// rather than cutting their titles.
struct WrapLayout: Layout {
  var spacing: CGFloat = 6

  func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
    let frames = Self.frames(width: proposal.width ?? .infinity, spacing: spacing, sizes: sizes(subviews, width: proposal.width))
    return CGSize(width: frames.map(\.maxX).max() ?? 0, height: frames.map(\.maxY).max() ?? 0)
  }

  func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
    let frames = Self.frames(width: bounds.width, spacing: spacing, sizes: sizes(subviews, width: bounds.width))
    for (frame, view) in zip(frames, subviews) {
      view.place(at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY), proposal: ProposedViewSize(frame.size))
    }
  }

  private func sizes(_ subviews: Subviews, width: CGFloat?) -> [CGSize] {
    subviews.map { $0.sizeThatFits(ProposedViewSize(width: width, height: nil)) }
  }

  /// Where each size lands in rows no wider than `width`; a view wider than the row gets a row of its own.
  static func frames(width: CGFloat, spacing: CGFloat, sizes: [CGSize]) -> [CGRect] {
    var frames: [CGRect] = []
    var point = CGPoint.zero
    var rowHeight: CGFloat = 0
    for size in sizes {
      if point.x > 0, point.x + size.width > width {
        point = CGPoint(x: 0, y: point.y + rowHeight + spacing)
        rowHeight = 0
      }
      frames.append(CGRect(origin: point, size: CGSize(width: min(size.width, width), height: size.height)))
      point.x += size.width + spacing
      rowHeight = max(rowHeight, size.height)
    }
    return frames
  }
}
