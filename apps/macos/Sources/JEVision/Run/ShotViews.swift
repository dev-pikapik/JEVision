import AppKit
import JEVisionCore
import SwiftUI

/// A screen compared with its reference: either picture, the differences, or one over the other.
struct Comparison: View {
  enum Mode: Hashable {
    case screen, reference, differences, overlay
  }

  let step: Step
  let images: StepImages
  let preview: (URL) -> Void
  @State private var mode: Mode
  /// How much of the screen shows over the reference.
  @State private var blend = 0.5

  init(step: Step, images: StepImages, preview: @escaping (URL) -> Void) {
    self.step = step
    self.images = images
    self.preview = preview
    _mode = State(initialValue: step.status == .failed && images.diff != nil ? .differences : .screen)
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      HStack(spacing: 12) {
        Picker("Show", selection: $mode) {
          Text("Screen").tag(Mode.screen)
          Text("Reference").tag(Mode.reference)
          if images.diff != nil { Text("Differences").tag(Mode.differences) }
          Text("Overlay").tag(Mode.overlay)
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .fixedSize()
        Spacer(minLength: 0)
        if let similarity = step.similarity {
          Text(similarityText(similarity))
            .font(.callout)
            .foregroundStyle(.secondary)
        }
      }
      picture
      if mode == .overlay {
        Slider(value: $blend) {
          Text("Screen over reference")
        } minimumValueLabel: {
          Text("Reference")
        } maximumValueLabel: {
          Text("Screen")
        }
        .frame(maxWidth: 420)
      }
    }
  }

  @ViewBuilder private var picture: some View {
    if let after = images.after, let expected = images.expected {
      switch mode {
      case .screen:
        ShotImage(url: after, label: String(localized: "What the screen showed"), preview: preview)
      case .reference:
        ShotImage(url: expected, label: String(localized: "Reference image"), preview: preview)
      case .differences:
        if let diff = images.diff {
          ShotImage(url: diff, label: String(localized: "Differences: what changed is red"), preview: preview)
        }
      case .overlay:
        ShotImage(
          url: after, over: (expected, blend), label: String(localized: "The screen over the reference image"),
          preview: preview)
      }
    }
  }

  private func similarityText(_ similarity: Double) -> String {
    // The failure states both numbers the way the runner rounded them.
    let params = step.failure?.params ?? [:]
    let matched = params["similarity"] ?? BlockSummary.percent(similarity)
    if let threshold = params["threshold"] {
      return String(localized: "Matches by \(matched), needs \(threshold)")
    }
    return String(localized: "Matches by \(matched)")
  }
}

/// A screenshot from the report, fitted to the space, with a caption. Space or a double-click
/// opens it in Quick Look.
struct ShotImage: View {
  let url: URL
  /// A picture shown on top, at the given opacity, to compare the two.
  var over: (base: URL, opacity: Double)?
  let label: String
  let preview: (URL) -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 4) {
      Text(label)
        .font(.caption.weight(.semibold))
        .foregroundStyle(.secondary)
        .accessibilityHidden(true)
      ZStack {
        if let over {
          Picture(url: over.base)
          Picture(url: url).opacity(over.opacity)
        } else {
          Picture(url: url)
        }
      }
      .clipShape(.rect(cornerRadius: 8))
      .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(.separator))
      .frame(maxHeight: 420, alignment: .topLeading)
      .focusable()
      .onKeyPress(.space) {
        preview(url)
        return .handled
      }
      .onTapGesture(count: 2) { preview(url) }
      .help(Text("Double-click or press Space to open in Quick Look"))
      .accessibilityElement(children: .ignore)
      .accessibilityLabel(Text(label))
      .accessibilityAddTraits(.isImage)
      .accessibilityHint(Text("Press Space to open in Quick Look"))
      .accessibilityAction(named: Text("Quick Look")) { preview(url) }
    }
  }
}

/// An image file, loaded once and scaled to fit.
private struct Picture: View {
  let url: URL
  @State private var image: NSImage?
  @State private var isMissing = false

  var body: some View {
    Group {
      if let image {
        Image(nsImage: image)
          .resizable()
          .interpolation(.high)
          .aspectRatio(contentMode: .fit)
      } else {
        Rectangle()
          .fill(.quaternary)
          .aspectRatio(16 / 10, contentMode: .fit)
          .overlay {
            if isMissing {
              Label("The image isn’t there anymore", systemImage: "photo.badge.exclamationmark")
                .foregroundStyle(.secondary)
            }
          }
      }
    }
    .task(id: url) {
      image = NSImage(contentsOf: url)
      isMissing = image == nil
    }
  }
}
