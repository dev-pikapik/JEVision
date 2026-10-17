import SwiftUI

/// Centered tile, title and one line of explanation at the top of a settings page or onboarding step.
struct PageHeader: View {
  let symbol: String
  let color: Color
  let title: Text
  let message: Text
  var tileSize: CGFloat = 52

  var body: some View {
    VStack(spacing: 0) {
      IconTile(symbol: symbol, color: color, size: tileSize)
        .padding(.bottom, 12)
      title
        .font(.title.bold())
        .padding(.bottom, 4)
        .accessibilityAddTraits(.isHeader)
      message
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
    }
    .multilineTextAlignment(.center)
    .frame(maxWidth: .infinity)
    .padding(.horizontal, 24)
    .padding(.vertical, 10)
  }
}
