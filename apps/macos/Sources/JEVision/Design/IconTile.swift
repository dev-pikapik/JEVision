import SwiftUI

/// A rounded glass tile with a symbol, used for headers, rows and empty states.
struct IconTile: View {
  let symbol: String
  let color: Color
  var size: CGFloat = 28

  var body: some View {
    Image(systemName: symbol)
      .font(.system(size: size * 0.48, weight: .semibold))
      .foregroundStyle(.white)
      .frame(width: size, height: size)
      .glassEffect(.regular.tint(color), in: .rect(cornerRadius: size * 0.27))
      .accessibilityHidden(true)
  }
}
