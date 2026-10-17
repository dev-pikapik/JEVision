import SwiftUI

struct WelcomeStep: View {
  var body: some View {
    VStack(spacing: 0) {
      IconTile(symbol: "flowchart.fill", color: .accentColor, size: 92)
        .padding(.bottom, 24)
      Text("Welcome to JEVision")
        .font(.largeTitle.bold())
        .accessibilityAddTraits(.isHeader)
        .padding(.bottom, 10)
      Text("Test any Mac app the way a person would: click, type, look. A test is a flowchart you can read — no code.")
        .font(.title3)
        .foregroundStyle(.secondary)
        .frame(maxWidth: 500)
        .fixedSize(horizontal: false, vertical: true)
    }
    .multilineTextAlignment(.center)
    .padding(40)
  }
}
