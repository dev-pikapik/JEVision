import JEVisionCore
import SwiftUI

/// A picture with the areas of `reference` over it, where the pointer draws new areas of kind
/// `tool`, moves areas and resizes the selected one by its corners; arrow keys nudge the
/// selected area and Delete removes it. Every change goes through `change`: changes with the
/// same non-nil kind in a row undo as one step.
struct AreaCanvas: View {
  let picture: ReferencePicture
  let reference: Reference
  let tool: AreaKind
  @Binding var selection: AreaID?
  var focus: FocusState<Bool>.Binding
  let label: Text
  let change: (String?, (inout Reference) -> Void) -> Void

  /// What the pointer does on the picture.
  private enum Action {
    case draw(from: CGPoint)
    case move(AreaID, Rect)
    case resize(AreaID, Rect, AreaDrag.Corner)
  }

  @State private var action: Action?
  /// The area being dragged, before it's saved.
  @State private var draft: (id: AreaID?, kind: AreaKind, rect: Rect)?
  @State private var hover: CGPoint?

  var body: some View {
    GeometryReader { geometry in
      let frame = ImageFrame(pixels: picture.pixels, bounds: geometry.size)
      ReferenceCanvas(picture: picture, reference: shown, selection: draft?.id ?? selection)
        .contentShape(.rect)
        .gesture(dragGesture(frame))
        .onContinuousHover { phase in
          if case .active(let point) = phase { hover = point } else { hover = nil }
        }
        .pointerStyle(pointer(frame))
        .overlay { accessibilityAreas(frame) }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(label)
    }
    .unmirrored()
    .padding(16)
    .focusable()
    .focusEffectDisabled()
    .focused(focus)
    .onKeyPress(keys: [.delete, .deleteForward]) { _ in
      guard let selection else { return .ignored }
      remove(selection)
      return .handled
    }
    .onKeyPress(keys: [.leftArrow, .rightArrow, .upArrow, .downArrow]) { press in
      guard let selection else { return .ignored }
      nudge(selection, press.key, by: press.modifiers.contains(.shift) ? 10 : 1)
      return .handled
    }
  }

  /// The reference as it looks right now, with the area being dragged in its new place.
  private var shown: Reference {
    guard let draft else { return reference }
    var r = reference
    if let id = draft.id { r[area: id] = draft.rect } else { r.add(draft.kind, draft.rect) }
    return r
  }

  /// What a press at `point` would do: resize the selected area by a corner, move an area, or draw.
  private func action(at point: CGPoint, _ frame: ImageFrame) -> Action {
    if let selection, let r = reference[area: selection] {
      let view = frame.view(r)
      for corner in AreaDrag.Corner.allCases {
        let p = corner.point(of: Rect(x: view.minX, y: view.minY, width: view.width, height: view.height))
        if hypot(p.x - point.x, p.y - point.y) <= 8 { return .resize(selection, r, corner) }
      }
    }
    // Areas drawn last are on top.
    for id in reference.areaIDs.reversed() {
      if let r = reference[area: id], frame.view(r).insetBy(dx: -2, dy: -2).contains(point) { return .move(id, r) }
    }
    return .draw(from: frame.fraction(point))
  }

  private func pointer(_ frame: ImageFrame) -> PointerStyle {
    guard let hover else { return .default }
    if case .move = action { return .grabActive }
    switch action ?? action(at: hover, frame) {
    case .draw: return frame.rect.contains(hover) ? .rectSelection : .default
    case .move: return .grabIdle
    case .resize(_, _, let corner):
      switch corner {
      case .topLeft: return .frameResize(position: .topLeading)
      case .topRight: return .frameResize(position: .topTrailing)
      case .bottomLeft: return .frameResize(position: .bottomLeading)
      case .bottomRight: return .frameResize(position: .bottomTrailing)
      }
    }
  }

  private func dragGesture(_ frame: ImageFrame) -> some Gesture {
    DragGesture(minimumDistance: 0)
      .onChanged { value in
        focus.wrappedValue = true
        let action = self.action ?? action(at: value.startLocation, frame)
        self.action = action
        switch action {
        case .draw(let start):
          draft = (nil, tool, frame.snapped(AreaDrag.rect(from: start, to: frame.fraction(value.location))))
        case .move(let id, let r):
          selection = id
          draft = (id, id.kind, frame.snapped(AreaDrag.moved(r, by: frame.fraction(value.translation))))
        case .resize(let id, let r, let corner):
          draft = (id, id.kind, frame.snapped(AreaDrag.resized(r, corner: corner, to: frame.fraction(value.location))))
        }
      }
      .onEnded { value in
        defer {
          action = nil
          draft = nil
        }
        guard let action = self.action, let draft else { return }
        switch action {
        case .draw:
          // A click without a drag only clears the selection.
          let moved = hypot(value.translation.width, value.translation.height) >= 4
          guard moved else {
            selection = nil
            return
          }
          change(nil) { selection = $0.add(draft.kind, draft.rect) }
        case .move(let id, let r), .resize(let id, let r, _):
          if draft.rect != r { change(nil) { $0[area: id] = draft.rect } }
        }
      }
  }

  /// One invisible element per area, so VoiceOver can find, select and delete each.
  private func accessibilityAreas(_ frame: ImageFrame) -> some View {
    ForEach(reference.areaIDs, id: \.self) { id in
      if let r = reference[area: id] {
        let view = frame.view(r)
        let pixels = frame.pixels(r)
        Color.clear
          .frame(width: view.width, height: view.height)
          .position(x: view.midX, y: view.midY)
          .allowsHitTesting(false)
          .accessibilityElement()
          .accessibilityLabel(Text(id.kind.name(id.index)))
          .accessibilityValue(Text("\(pixels.width) × \(pixels.height) px at \(pixels.x), \(pixels.y)"))
          .accessibilityAddTraits(selection == id ? [.isSelected, .isButton] : .isButton)
          .accessibilityAction { selection = id }
          .accessibilityAction(named: Text("Delete")) { remove(id) }
      }
    }
  }

  private func remove(_ id: AreaID) {
    change(nil) { $0.remove(id) }
    selection = nil
  }

  private func nudge(_ id: AreaID, _ key: KeyEquivalent, by step: Int) {
    guard let r = reference[area: id] else { return }
    let frame = ImageFrame(pixels: picture.pixels, bounds: picture.pixels)
    var p = frame.pixels(r)
    switch key {
    case .leftArrow: p.x = max(p.x - step, 0)
    case .rightArrow: p.x = min(p.x + step, Int(picture.pixels.width) - p.width)
    case .upArrow: p.y = max(p.y - step, 0)
    default: p.y = min(p.y + step, Int(picture.pixels.height) - p.height)
    }
    change("nudge.\(id)") { $0[area: id] = frame.fraction(p) }
  }
}

/// One area in a list: its name and size, selected by a click, with a delete button.
struct AreaRow: View {
  let id: AreaID
  let rect: Rect
  /// The picture the areas are drawn on, to give the size in pixels; percents without one.
  let picture: ReferencePicture?
  let isSelected: Bool
  let select: () -> Void
  let delete: () -> Void

  var body: some View {
    HStack(spacing: 8) {
      AreaSwatch(kind: id.kind)
      VStack(alignment: .leading, spacing: 1) {
        Text(id.kind.name(id.index))
        Group {
          if let picture {
            let pixels = ImageFrame(pixels: picture.pixels, bounds: picture.pixels).pixels(rect)
            Text("\(pixels.width) × \(pixels.height) px at \(pixels.x), \(pixels.y)")
          } else {
            Text(
              "\(rect.width.formatted(.percent.precision(.fractionLength(0...1)))) × \(rect.height.formatted(.percent.precision(.fractionLength(0...1)))) of the window at \(rect.x.formatted(.percent.precision(.fractionLength(0...1)))), \(rect.y.formatted(.percent.precision(.fractionLength(0...1))))"
            )
          }
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        .monospacedDigit()
      }
      Spacer()
      Button(action: delete) {
        Image(systemName: "trash")
      }
      .buttonStyle(.borderless)
      .accessibilityLabel(Text("Delete \(id.kind.name(id.index))"))
    }
    .contentShape(.rect)
    .onTapGesture(perform: select)
    .listRowBackground(isSelected ? Color.accentColor.opacity(0.15) : nil)
    .accessibilityElement(children: .combine)
    .accessibilityAddTraits(isSelected ? [.isSelected, .isButton] : .isButton)
    .accessibilityAction { select() }
    .accessibilityAction(named: Text("Delete")) { delete() }
  }
}

/// Fields for the place and size of an area in whole pixels of the picture.
struct AreaPixelFields: View {
  let rect: Rect
  let frame: ImageFrame
  let change: (Rect) -> Void

  var body: some View {
    let pixels = frame.pixels(rect)
    field("From Left, px", \.x, pixels)
    field("From Top, px", \.y, pixels)
    field("Width, px", \.width, pixels)
    field("Height, px", \.height, pixels)
  }

  private func field(_ label: LocalizedStringKey, _ path: WritableKeyPath<PixelRect, Int>, _ pixels: PixelRect) -> some View {
    TextField(
      value: Binding(
        get: { pixels[keyPath: path] },
        set: { value in
          var p = pixels
          p[keyPath: path] = value
          change(frame.fraction(p))
        }),
      format: .number
    ) { Text(label) }
    .monospacedDigit()
  }
}
