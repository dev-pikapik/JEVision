import SwiftUI

/// A label with its buttons. The buttons sit at the trailing edge while the label keeps room to
/// read; when a longer translation or a larger font leaves it less than `ActionRow.labelWidth`,
/// they move under it and wrap, so no title is cut.
struct ActionRow<Label: View, Actions: View>: View {
  /// The width the label keeps beside its buttons.
  static var labelWidth: CGFloat { 200 }

  @ViewBuilder var actions: Actions
  @ViewBuilder var label: Label

  var body: some View {
    ActionRowLayout(labelWidth: Self.labelWidth) {
      VStack(alignment: .leading, spacing: 0) { label }
      WrapLayout(spacing: 8) { actions }
    }
  }
}

struct ActionRowLayout: Layout {
  var labelWidth: CGFloat
  var spacing: CGFloat = 12

  /// Whether buttons `actions` wide leave the label its room in a row `width` wide.
  func sitsBeside(actions: CGFloat, width: CGFloat) -> Bool {
    actions + spacing + labelWidth <= width
  }

  /// The label's and the buttons' frames inside a row `width` wide, and whether the buttons sit beside the label.
  func arrange(width: CGFloat, subviews: Subviews) -> (label: CGRect, actions: CGRect, beside: Bool) {
    let ideal = subviews[1].sizeThatFits(.unspecified)
    if sitsBeside(actions: ideal.width, width: width) {
      let label = subviews[0].sizeThatFits(ProposedViewSize(width: width - ideal.width - spacing, height: nil))
      let height = max(label.height, ideal.height)
      return (
        CGRect(x: 0, y: (height - label.height) / 2, width: label.width, height: label.height),
        CGRect(x: width - ideal.width, y: (height - ideal.height) / 2, width: ideal.width, height: ideal.height),
        true
      )
    }
    let label = subviews[0].sizeThatFits(ProposedViewSize(width: width, height: nil))
    let actions = subviews[1].sizeThatFits(ProposedViewSize(width: width, height: nil))
    return (
      CGRect(x: 0, y: 0, width: label.width, height: label.height),
      CGRect(x: 0, y: label.height + 6, width: actions.width, height: actions.height),
      false
    )
  }

  func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
    guard let width = proposal.width, width.isFinite else {
      let label = subviews[0].sizeThatFits(.unspecified)
      let actions = subviews[1].sizeThatFits(.unspecified)
      return CGSize(width: label.width + spacing + actions.width, height: max(label.height, actions.height))
    }
    let frames = arrange(width: width, subviews: subviews)
    return CGSize(width: width, height: max(frames.label.maxY, frames.actions.maxY))
  }

  func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
    let frames = arrange(width: bounds.width, subviews: subviews)
    for (frame, view) in zip([frames.label, frames.actions], subviews) {
      view.place(at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY), proposal: ProposedViewSize(frame.size))
    }
  }
}
