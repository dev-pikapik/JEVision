import SwiftUI

struct AIStep: View {
  var body: some View {
    VStack(spacing: 0) {
      PageHeader(
        symbol: "sparkles", color: .purple, title: Text("Checks by meaning"),
        message: Text("Optional. An AI model can check a screen from a sentence like “The cart shows 2 items”. Choose one now, or later in Settings → AI."),
        tileSize: 64)
      Form {
        AISections()
      }
      .formStyle(.grouped)
      .scrollContentBackground(.hidden)
    }
    .padding(.horizontal, 32)
    .padding(.top, 28)
  }
}
