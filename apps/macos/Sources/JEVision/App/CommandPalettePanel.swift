import AppKit
import SwiftUI

/// Shows the command palette in a panel of its own over the window this view is in. The panel
/// becomes the key window the way a popover does, so the search field has the keyboard at once
/// and Escape and the arrow keys reach the palette. Drawn as an overlay inside the window, the
/// palette had to win focus from the canvas, the sidebar and the inspector, and lost to them.
struct CommandPalettePresenter<Palette: View>: NSViewRepresentable {
  let isPresented: Bool
  /// Asks for the palette to close: Escape, a click outside it, another window or app in front.
  let dismiss: @MainActor () -> Void
  @ViewBuilder let palette: () -> Palette

  func makeNSView(context: Context) -> NSView { Anchor() }

  func makeCoordinator() -> Coordinator { Coordinator() }

  func updateNSView(_ view: NSView, context: Context) {
    let coordinator = context.coordinator
    if isPresented, coordinator.panel == nil, let window = view.window {
      coordinator.panel = CommandPalettePanel(palette(), over: window, dismiss: dismiss)
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
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
  }
}

/// A borderless panel, attached to the window above its content, that holds the palette.
final class CommandPalettePanel: NSPanel {
  /// How far below the toolbar the palette's top edge is.
  static let top = 72.0
  private var dismiss: (@MainActor () -> Void)?

  init(_ palette: some View, over window: NSWindow, dismiss: @escaping @MainActor () -> Void) {
    let size = CommandPaletteView.size
    let area = window.convertToScreen(window.contentLayoutRect)
    super.init(
      contentRect: NSRect(
        x: area.midX - size.width / 2, y: area.maxY - Self.top - size.height, width: size.width, height: size.height),
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
  }

  override var canBecomeKey: Bool { true }

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
    parent?.removeChildWindow(self)
    super.close()
  }
}
