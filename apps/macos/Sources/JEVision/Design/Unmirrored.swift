import SwiftUI

extension View {
  /// Keeps left and right where they are when the language reads right to left. Positions in a
  /// test's flowchart, a board, a screenshot or a video timeline are data, not text layout.
  func unmirrored() -> some View {
    environment(\.layoutDirection, .leftToRight)
  }
}
