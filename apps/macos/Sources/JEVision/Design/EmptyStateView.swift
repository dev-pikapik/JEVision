import SwiftUI

/// One icon, one title, one sentence and one button, with an optional quieter second one.
struct EmptyStateView: View {
  let symbol: String
  let title: Text
  let message: Text
  let action: Text
  let perform: () -> Void
  var secondAction: Text?
  var performSecond: () -> Void = {}

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
      if let secondAction {
        Button(action: performSecond) { secondAction }
          .buttonStyle(.glass)
          .controlSize(.large)
      }
    }
  }
}
