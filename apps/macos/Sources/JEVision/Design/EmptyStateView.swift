import SwiftUI

/// One icon, one title, one sentence and one button.
struct EmptyStateView: View {
  let symbol: String
  let title: Text
  let message: Text
  let action: Text
  let perform: () -> Void

  var body: some View {
    ContentUnavailableView {
      Label { title } icon: {
        IconTile(symbol: symbol, color: .accentColor, size: 84)
      }
    } description: {
      message
        .frame(maxWidth: 420)
    } actions: {
      Button(action: perform) { action }
        .buttonStyle(.glassProminent)
        .controlSize(.large)
        .keyboardShortcut(.defaultAction)
    }
  }
}
