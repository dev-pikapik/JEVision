import SwiftUI

/// A rounded colour tile with a white symbol, used for blocks, headers, rows and empty states.
/// A solid fill, not glass: tiles are content, and stay legible in inactive windows and light mode.
struct IconTile: View {
  let symbol: String
  let color: Color
  var size: CGFloat = 28

  var body: some View {
    Image(systemName: symbol)
      .font(.system(size: size * 0.48, weight: .semibold))
      .foregroundStyle(.white)
      .frame(width: size, height: size)
      .background(color.gradient, in: .rect(cornerRadius: size * 0.27))
      .accessibilityHidden(true)
  }
}
