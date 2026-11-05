import AppKit
import SwiftUI

/// Space covered by controls, independent of the canvas beneath them.
struct WorkspaceInsets: Equatable, Sendable {
  var leading: CGFloat = 0
  var trailing: CGFloat = 0
  var top: CGFloat = 0

  func workingArea(in size: CGSize) -> CGRect {
    CGRect(x: leading, y: top, width: max(1, size.width - leading - trailing),
      height: max(1, size.height - top))
  }
}

private struct WorkspaceInsetsKey: EnvironmentKey {
  static let defaultValue = WorkspaceInsets()
}

extension EnvironmentValues {
  var workspaceInsets: WorkspaceInsets {
    get { self[WorkspaceInsetsKey.self] }
    set { self[WorkspaceInsetsKey.self] = newValue }
  }
}

extension View {
  /// Panel positions follow the physical canvas axes; their controls keep the app's text direction.
  func workspacePadding(leading: CGFloat = 0, trailing: CGFloat = 0) -> some View {
    modifier(WorkspacePadding(leading: leading, trailing: trailing))
  }
}

private struct WorkspacePadding: ViewModifier {
  let leading: CGFloat
  let trailing: CGFloat
  @Environment(\.layoutDirection) private var direction

  func body(content: Content) -> some View {
    content.environment(\.layoutDirection, direction)
      .padding(.leading, leading).padding(.trailing, trailing)
      .environment(\.layoutDirection, .leftToRight)
  }
}

/// Panel preferences remain useful after resizing the window; a compact window still leaves
/// room to work. `available` excludes any panel on the opposite side.
enum WorkspacePanelWidth {
  static func clamp(
    _ preferred: Double, minimum: Double, maximum: Double, available: Double, reserving: Double = 320
  ) -> Double {
    let room = max(0, (available.isFinite ? available : 0) - max(0, reserving))
    let upper = min(maximum, room)
    let lower = min(minimum, upper)
    return min(upper, max(lower, preferred.isFinite ? preferred : minimum))
  }

  /// A panel's right edge grows toward the right; its left edge grows toward the left.
  static func resized(
    from initial: Double, by translation: Double, trailingEdge: Bool,
    minimum: Double, maximum: Double, available: Double
  ) -> Double {
    clamp(initial + (trailingEdge ? translation : -translation),
      minimum: minimum, maximum: maximum, available: available)
  }
}

/// A divider that changes a panel's width without relaying out the drawing behind it.
struct WorkspacePanelResizer: View {
  @Binding var width: Double
  let minimum: Double
  let maximum: Double
  let available: Double
  /// `true` for the right edge of the leading panel, `false` for the left edge of the trailing one.
  let trailingEdge: Bool
  let label: Text
  @State private var initial: Double?
  @State private var hovered = false

  var body: some View {
    WorkspacePanelResizeHandle(
      hovered: $hovered,
      start: { initial = constrained(width) },
      drag: { delta in
        if initial == nil { initial = constrained(width) }
        width = WorkspacePanelWidth.resized(from: initial ?? width, by: delta,
          trailingEdge: trailingEdge, minimum: minimum, maximum: maximum, available: available)
      },
      end: { initial = nil }
    )
      .frame(width: 10)
      .frame(maxHeight: .infinity)
      .padding(.vertical, 16)
      .contentShape(.rect)
      .overlay {
        Capsule().fill(.secondary.opacity(hovered ? 0.65 : 0.25))
          .frame(width: 3, height: 36)
          .allowsHitTesting(false)
      }
      .gesture(DragGesture(minimumDistance: 0, coordinateSpace: .global)
        .onChanged { value in
          if initial == nil { initial = constrained(width) }
          width = WorkspacePanelWidth.resized(from: initial ?? width, by: value.translation.width,
            trailingEdge: trailingEdge, minimum: minimum, maximum: maximum, available: available)
        }
        .onEnded { _ in initial = nil })
      .focusable()
      .onKeyPress(.leftArrow) { adjust(trailingEdge ? -20 : 20); return .handled }
      .onKeyPress(.rightArrow) { adjust(trailingEdge ? 20 : -20); return .handled }
      .accessibilityElement()
      .accessibilityLabel(label)
      .accessibilityValue(Text(constrained(width), format: .number.precision(.fractionLength(0))))
      .accessibilityAdjustableAction { direction in adjust(direction == .increment ? 20 : -20) }
  }

  private func constrained(_ value: Double) -> Double {
    WorkspacePanelWidth.clamp(value, minimum: minimum, maximum: maximum, available: available)
  }

  private func adjust(_ delta: Double) { width = constrained(constrained(width) + delta) }
}

/// AppKit resize strip that receives pointer drags above scroll views and tracks in window space.
struct WorkspacePanelResizeHandle: NSViewRepresentable {
  @Binding var hovered: Bool
  let start: () -> Void
  let drag: (Double) -> Void
  let end: () -> Void

  func makeNSView(context: Context) -> HandleView {
    let view = HandleView()
    view.configure(hovered: { hovered = $0 }, start: start, drag: drag, end: end)
    return view
  }

  func updateNSView(_ view: HandleView, context: Context) {
    view.configure(hovered: { hovered = $0 }, start: start, drag: drag, end: end)
  }

  @MainActor final class HandleView: NSView {
    private var onHover: (Bool) -> Void = { _ in }
    private var onStart: () -> Void = {}
    private var onDrag: (Double) -> Void = { _ in }
    private var onEnd: () -> Void = {}
    private var tracking: NSTrackingArea?
    private var startX: CGFloat?

    func configure(
      hovered: @escaping (Bool) -> Void,
      start: @escaping () -> Void,
      drag: @escaping (Double) -> Void,
      end: @escaping () -> Void
    ) {
      onHover = hovered
      onStart = start
      onDrag = drag
      onEnd = end
    }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override var mouseDownCanMoveWindow: Bool { false }

    override func resetCursorRects() {
      addCursorRect(bounds, cursor: .resizeLeftRight)
    }

    override func updateTrackingAreas() {
      super.updateTrackingAreas()
      if let tracking { removeTrackingArea(tracking) }
      let area = NSTrackingArea(
        rect: bounds,
        options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect, .cursorUpdate],
        owner: self, userInfo: nil)
      addTrackingArea(area)
      tracking = area
    }

    override func cursorUpdate(with event: NSEvent) {
      NSCursor.resizeLeftRight.set()
    }

    override func mouseEntered(with event: NSEvent) {
      onHover(true)
      NSCursor.resizeLeftRight.set()
    }

    override func mouseExited(with event: NSEvent) {
      if startX == nil { onHover(false) }
    }

    override func mouseDown(with event: NSEvent) {
      beginDrag(atWindowX: event.locationInWindow.x)
    }

    override func mouseDragged(with event: NSEvent) {
      continueDrag(toWindowX: event.locationInWindow.x)
    }

    override func mouseUp(with event: NSEvent) {
      finishDrag()
    }

    func beginDrag(atWindowX x: CGFloat) {
      startX = x
      onHover(true)
      NSCursor.resizeLeftRight.set()
      onStart()
    }

    func continueDrag(toWindowX x: CGFloat) {
      if startX == nil { beginDrag(atWindowX: x) }
      guard let startX else { return }
      onDrag(Double(x - startX))
    }

    func finishDrag() {
      startX = nil
      onEnd()
    }
  }
}

/// A native search field for a panel that floats over the workspace.
struct WorkspaceSearchField: NSViewRepresentable {
  @Binding var text: String
  let prompt: String

  func makeCoordinator() -> Coordinator { Coordinator(self) }

  func makeNSView(context: Context) -> NSSearchField {
    context.coordinator.makeField()
  }

  func updateNSView(_ field: NSSearchField, context: Context) {
    context.coordinator.parent = self
    if field.stringValue != text { field.stringValue = text }
    field.placeholderString = prompt
    field.setAccessibilityLabel(prompt)
  }

  @MainActor final class ModernSearchFieldCell: NSSearchFieldCell {
    override func searchTextRect(forBounds rect: NSRect) -> NSRect {
      var r = super.searchTextRect(forBounds: rect)
      r.origin.x += 2
      r.size.width -= 4
      return r
    }

    override func searchButtonRect(forBounds rect: NSRect) -> NSRect {
      var r = super.searchButtonRect(forBounds: rect)
      r.origin.x += 4
      return r
    }

    override func cancelButtonRect(forBounds rect: NSRect) -> NSRect {
      var r = super.cancelButtonRect(forBounds: rect)
      r.origin.x -= 4
      return r
    }
  }

  @MainActor final class ModernSearchField: NSSearchField {
    override class var cellClass: AnyClass? {
      get { ModernSearchFieldCell.self }
      set { super.cellClass = newValue }
    }

    override init(frame frameRect: NSRect) {
      super.init(frame: frameRect)
      commonInit()
    }

    required init?(coder: NSCoder) {
      super.init(coder: coder)
      commonInit()
    }

    private func commonInit() {
      wantsLayer = true
      isBezeled = false
      isBordered = false
      drawsBackground = false
      focusRingType = .exterior
      font = .systemFont(ofSize: 13)
    }

    override func updateLayer() {
      super.updateLayer()
      layer?.cornerRadius = 8
      layer?.cornerCurve = .continuous
      let isDark = effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
      layer?.backgroundColor = isDark
        ? NSColor.white.withAlphaComponent(0.08).cgColor
        : NSColor.black.withAlphaComponent(0.05).cgColor
      layer?.borderColor = isDark
        ? NSColor.white.withAlphaComponent(0.12).cgColor
        : NSColor.black.withAlphaComponent(0.10).cgColor
      layer?.borderWidth = 0.5
    }
  }

  @MainActor final class Coordinator: NSObject, NSSearchFieldDelegate {
    var parent: WorkspaceSearchField
    init(_ parent: WorkspaceSearchField) { self.parent = parent }

    func makeField() -> NSSearchField {
      let field = ModernSearchField()
      field.delegate = self
      field.target = self
      field.action = #selector(changed(_:))
      field.sendsSearchStringImmediately = true
      field.sendsWholeSearchString = false
      field.setContentHuggingPriority(.defaultLow, for: .horizontal)
      return field
    }

    @objc func changed(_ sender: NSSearchField) { update(sender) }

    func controlTextDidChange(_ notification: Notification) {
      if let field = notification.object as? NSSearchField { update(field) }
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy command: Selector) -> Bool {
      guard command == #selector(NSResponder.cancelOperation(_:)),
        let field = control as? NSSearchField, !field.stringValue.isEmpty
      else { return false }
      field.stringValue = ""
      textView.string = ""
      update(field)
      return true
    }

    private func update(_ field: NSSearchField) {
      if parent.text != field.stringValue { parent.text = field.stringValue }
    }
  }
}
