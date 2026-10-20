import SwiftUI

/// A glass button for the floating panels. They are never the key window, and the system glass
/// button loses its tint in a window that isn't key, so a Stop button would be grey. Here the
/// colour sits under the glass and stays.
struct HUDButtonStyle: ButtonStyle {
  /// The colour of the button, `nil` for plain glass.
  var tint: Color?
  /// Whether the button takes all the width it is offered.
  var fillsWidth = false

  func makeBody(configuration: Configuration) -> some View {
    Face(configuration: configuration, tint: tint, fillsWidth: fillsWidth)
  }

  private struct Face: View {
    let configuration: ButtonStyleConfiguration
    let tint: Color?
    let fillsWidth: Bool
    @Environment(\.isEnabled) private var isEnabled

    var body: some View {
      configuration.label
        .lineLimit(1)
        .frame(maxWidth: fillsWidth ? .infinity : nil)
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .foregroundStyle(tint == nil ? AnyShapeStyle(.primary) : AnyShapeStyle(.white))
        .background { if let tint { Capsule().fill(tint) } }
        .adaptiveGlass(in: Capsule())
        .contentShape(Capsule())
        .opacity(isEnabled ? (configuration.isPressed ? 0.7 : 1) : 0.4)
    }
  }
}

/// Buttons of a floating panel in one row, some at each edge. When a longer translation leaves
/// them no room side by side, they wrap instead of squeezing their titles.
struct HUDButtonRow<Leading: View, Trailing: View>: View {
  @ViewBuilder var leading: Leading
  @ViewBuilder var trailing: Trailing

  var body: some View {
    ViewThatFits(in: .horizontal) {
      HStack(spacing: 8) {
        leading
        Spacer(minLength: 0)
        trailing
      }
      WrapLayout(spacing: 8) {
        leading
        trailing
      }
    }
  }
}

extension HUDButtonRow where Trailing == EmptyView {
  init(@ViewBuilder leading: () -> Leading) {
    self.init(leading: leading, trailing: { EmptyView() })
  }
}
