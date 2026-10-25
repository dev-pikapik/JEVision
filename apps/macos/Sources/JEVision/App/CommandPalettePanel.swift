import AppKit
import JEVisionHands
import SwiftUI

/// Shows the command palette in a panel of its own over the window this view is in. The panel
/// becomes the key window the way a popover does, so the search field has the keyboard at once
/// and Escape and the arrow keys reach the palette. Drawn as an overlay inside the window, the
/// palette had to win focus from the canvas, the sidebar and the inspector, and lost to them.
struct CommandPalettePresenter<Palette: View>: NSViewRepresentable {
  let isPresented: Bool
  var size = CommandPaletteView.size
  /// A top-left point in the anchor view; nil keeps the command palette centered.
  var location: CGPoint?
  @Environment(\.locale) private var locale
  @Environment(\.layoutDirection) private var direction
  /// Asks for the palette to close: Escape, a click outside it, another window or app in front.
  let dismiss: @MainActor () -> Void
  @ViewBuilder let palette: () -> Palette

  func makeNSView(context: Context) -> NSView { Anchor() }

  func makeCoordinator() -> Coordinator { Coordinator() }

  func updateNSView(_ view: NSView, context: Context) {
    let coordinator = context.coordinator
    if isPresented, coordinator.panel == nil, let window = view.window {
      let point = location.map { window.convertPoint(toScreen: view.convert($0, to: nil)) }
      coordinator.panel = CommandPalettePanel(
        palette().environment(\.locale, locale).environment(\.layoutDirection, direction)
          .appPreferences(Profile.current.defaults),
        size: size, topLeft: point, over: window, dismiss: dismiss)
    } else if !isPresented, let panel = coordinator.panel {
      coordinator.panel = nil
      panel.close()
    }
  }

  static func dismantleNSView(_ view: NSView, coordinator: Coordinator) {
    coordinator.panel?.close()
  }

  final class Coordinator {
    var panel: CommandPalettePanel?
  }

  /// Only tells which window to attach the panel to; clicks go through it.
  final class Anchor: NSView {
    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
  }
}

/// A borderless panel, attached to the window above its content, that holds the palette.
final class CommandPalettePanel: NSPanel {
  /// How far below the toolbar the palette's top edge is.
  static let top = 72.0
  private var dismiss: (@MainActor () -> Void)?
  private var outsideMonitor: Any?

  init(
    _ palette: some View, size: CGSize = CommandPaletteView.size, topLeft: CGPoint? = nil,
    over window: NSWindow, dismiss: @escaping @MainActor () -> Void
  ) {
    let area = window.convertToScreen(window.contentLayoutRect)
    let frame = Self.placement(size: size, topLeft: topLeft ?? CGPoint(x: area.midX - size.width / 2, y: area.maxY - Self.top),
      visible: window.screen?.visibleFrame ?? area)
    super.init(
      contentRect: frame,
      styleMask: [.borderless], backing: .buffered, defer: false)
    self.dismiss = dismiss
    isReleasedWhenClosed = false
    isOpaque = false
    backgroundColor = .clear
    hasShadow = false
    // The palette is only as tall as its results: clicks below them reach the window and close it.
    contentView = NSHostingView(rootView: palette.frame(width: size.width, height: size.height, alignment: .top))
    window.addChildWindow(self, ordered: .above)
    makeKey()
    outsideMonitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown]) { [weak self] event in
      if let self, event.window !== self { self.dismiss?() }
      return event
    }
  }

  override var canBecomeKey: Bool { true }

  static func placement(size: CGSize, topLeft: CGPoint, visible: CGRect) -> CGRect {
    let margin = 12.0
    return CGRect(
      x: min(max(topLeft.x, visible.minX + margin), max(visible.minX + margin, visible.maxX - size.width - margin)),
      y: min(max(topLeft.y - size.height, visible.minY + margin), max(visible.minY + margin, visible.maxY - size.height - margin)),
      width: size.width, height: size.height)
  }

  override func sendEvent(_ event: NSEvent) {
    if event.type == .keyDown, event.keyCode == 53 {
      dismiss?()
      return
    }
    super.sendEvent(event)
  }

  override func resignKey() {
    super.resignKey()
    dismiss?()
  }

  override func cancelOperation(_ sender: Any?) {
    dismiss?()
  }

  override func close() {
    // Closing on request doesn't ask again.
    dismiss = nil
    if let outsideMonitor { NSEvent.removeMonitor(outsideMonitor) }
    outsideMonitor = nil
    parent?.removeChildWindow(self)
    super.close()
  }
}
