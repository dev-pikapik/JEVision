import JEVisionCore
import SwiftUI

/// Two versions of a picture: side by side, under a sliding divider, one over the other, or
/// with the pixels that differ in red.
struct PictureDiffView: View {
  enum Mode: Hashable {
    case sideBySide, slider, overlay, differences
  }

  let before: Data?
  let after: Data?
  @State private var mode = Mode.sideBySide
  /// Where the divider is, from the left edge, in the slider.
  @State private var split = 0.5
  /// How much of the newer version shows over the older one.
  @State private var blend = 0.5
  @State private var diff: PictureDiff?

  var body: some View {
    let old = before.flatMap(NSImage.init(data:))
    let new = after.flatMap(NSImage.init(data:))
    ScrollView {
      VStack(alignment: .leading, spacing: 12) {
        if let old, let new {
          HStack {
            Picker("Show", selection: $mode) {
              Text("Side by Side").tag(Mode.sideBySide)
              Text("Slider").tag(Mode.slider)
              Text("Overlay").tag(Mode.overlay)
              if diff != nil { Text("Differences").tag(Mode.differences) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
            Spacer(minLength: 0)
            if let diff {
              Text(diff.differing == 0 ? String(localized: "The pictures are the same") : String(localized: "Same by \(BlockSummary.percent(diff.similarity))"))
                .font(.callout)
                .foregroundStyle(.secondary)
            }
          }
          compare(old, new)
        } else if let new {
          picture(new, label: Text("Added"))
        } else if let old {
          picture(old, label: Text("Deleted"))
        } else {
          Text("This version can’t be shown as a picture.")
            .foregroundStyle(.secondary)
        }
      }
      .padding()
    }
    .task(id: [before, after]) {
      diff = nil
      guard let before, let after else { return }
      let found = await Task.detached(priority: .userInitiated) { diffPictures(before: before, after: after) }.value
      guard !Task.isCancelled else { return }
      diff = found
      if found == nil, mode == .differences { mode = .sideBySide }
    }
  }

  @ViewBuilder
  private func compare(_ old: NSImage, _ new: NSImage) -> some View {
    switch mode {
    case .sideBySide:
      HStack(alignment: .top, spacing: 12) {
        picture(old, label: Text("Before"))
        picture(new, label: Text("After"))
      }
    case .slider:
      VStack(alignment: .leading, spacing: 8) {
        frame(label: Text("Before on the left, after on the right")) {
          image(old)
            .overlay {
              GeometryReader { proxy in
                image(new)
                  .mask(alignment: .trailing) { Rectangle().frame(width: proxy.size.width * (1 - split)) }
                Rectangle()
                  .fill(.white)
                  .frame(width: 2)
                  .shadow(radius: 2)
                  .offset(x: proxy.size.width * split - 1)
                Color.clear
                  .contentShape(.rect)
                  .gesture(DragGesture(minimumDistance: 0).onChanged { value in
                    split = min(1, max(0, value.location.x / max(proxy.size.width, 1)))
                  })
              }
            }
        }
        Slider(value: $split) {
          Text("Divider")
        } minimumValueLabel: {
          Text("After")
        } maximumValueLabel: {
          Text("Before")
        }
        .frame(maxWidth: 420)
      }
      .unmirrored()
    case .overlay:
      VStack(alignment: .leading, spacing: 8) {
        frame(label: Text("After over before")) {
          image(old).overlay { image(new).opacity(blend) }
        }
        Slider(value: $blend) {
          Text("After over before")
        } minimumValueLabel: {
          Text("Before")
        } maximumValueLabel: {
          Text("After")
        }
        .frame(maxWidth: 420)
      }
    case .differences:
      if let diff, let marked = NSImage(data: diff.png) {
        picture(marked, label: Text("Differences: what changed is red"))
      }
    }
  }

  private func picture(_ nsImage: NSImage, label: Text) -> some View {
    frame(label: label) { image(nsImage) }
  }

  private func image(_ nsImage: NSImage) -> some View {
    Image(nsImage: nsImage)
      .resizable()
      .interpolation(.high)
      .aspectRatio(contentMode: .fit)
  }

  private func frame(label: Text, @ViewBuilder content: () -> some View) -> some View {
    VStack(alignment: .leading, spacing: 4) {
      label
        .font(.caption.weight(.semibold))
        .foregroundStyle(.secondary)
        .accessibilityHidden(true)
      content()
        .clipShape(.rect(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(.separator))
        .frame(maxHeight: 420, alignment: .topLeading)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(label)
        .accessibilityAddTraits(.isImage)
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }
}
