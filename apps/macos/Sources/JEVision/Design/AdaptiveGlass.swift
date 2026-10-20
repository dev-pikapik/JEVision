import SwiftUI

extension View {
  /// Liquid Glass that follows the accessibility display settings: with Reduce Transparency it
  /// becomes a solid surface, with Increase Contrast it gets a clear border.
  func adaptiveGlass(tint: Color? = nil, in shape: some InsettableShape = Capsule()) -> some View {
    modifier(AdaptiveGlass(tint: tint, shape: shape))
  }
}

private struct AdaptiveGlass<S: InsettableShape>: ViewModifier {
  let tint: Color?
  let shape: S
  @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
  @Environment(\.colorSchemeContrast) private var contrast

  func body(content: Content) -> some View {
    Group {
      if reduceTransparency {
        content.background {
          shape.fill(.background)
          if let tint { shape.fill(tint) }
        }
      } else {
        content.glassEffect(tint.map { .regular.tint($0) } ?? .regular, in: shape)
      }
    }
    .overlay {
      // The border is only drawn: clicks go to the buttons under it.
      Group {
        if contrast == .increased {
          shape.strokeBorder(.primary.opacity(0.6), lineWidth: 1)
        } else if reduceTransparency {
          shape.strokeBorder(.separator, lineWidth: 1)
        }
      }
      .allowsHitTesting(false)
    }
  }
}
