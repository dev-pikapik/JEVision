import AppKit
import SwiftUI

/// The workspace remains visible through its toolbar until the pointer reaches the controls.
struct WorkspaceChrome: ViewModifier {
  @State private var hovered = false
  @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
  @Environment(\.colorSchemeContrast) private var contrast

  func body(content: Content) -> some View {
    content
      .background(ToolbarHover { hovered = $0 })
      .toolbarBackgroundVisibility(hovered || reduceTransparency || contrast == .increased ? .visible : .hidden, for: .windowToolbar)
  }
}

private struct ToolbarHover: NSViewRepresentable {
  let changed: (Bool) -> Void

  func makeNSView(context: Context) -> Hook { Hook() }
  func updateNSView(_ view: Hook, context: Context) { view.changed = changed }

  static func dismantleNSView(_ view: Hook, coordinator: ()) { view.stop() }

  final class Hook: NSView {
    var changed: ((Bool) -> Void)?
    private weak var trackingHost: NSView?
    private var tracking: NSTrackingArea?
    private var resizeObserver: NSObjectProtocol?
    private var hovered = false

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func viewDidMoveToWindow() {
      super.viewDidMoveToWindow()
      stop()
      guard let window else { return }
      window.styleMask.insert(.fullSizeContentView)
      window.toolbarStyle = .unified
      window.titlebarSeparatorStyle = .none
      installTracking()
      resizeObserver = NotificationCenter.default.addObserver(forName: NSWindow.didResizeNotification,
        object: window, queue: .main) { [weak self] _ in
        MainActor.assumeIsolated { self?.installTracking() }
      }
    }

    override func updateTrackingAreas() {
      super.updateTrackingAreas()
      installTracking()
    }

    private func installTracking() {
      if let tracking { trackingHost?.removeTrackingArea(tracking) }
      tracking = nil
      guard let window, let host = window.contentView?.superview else { return }
      trackingHost = host
      let top = window.contentLayoutRect.maxY
      let band = NSRect(x: 0, y: top, width: window.frame.width, height: max(0, host.bounds.height - top))
      let area = NSTrackingArea(rect: host.convert(band, from: nil),
        options: [.activeAlways, .mouseEnteredAndExited], owner: self, userInfo: nil)
      host.addTrackingArea(area)
      tracking = area
    }

    override func mouseEntered(with event: NSEvent) { setHovered(true) }
    override func mouseExited(with event: NSEvent) { setHovered(false) }

    private func setHovered(_ value: Bool) {
      guard value != hovered else { return }
      hovered = value
      changed?(value)
    }

    func stop() {
      if let tracking { trackingHost?.removeTrackingArea(tracking) }
      tracking = nil
      trackingHost = nil
      if let resizeObserver { NotificationCenter.default.removeObserver(resizeObserver) }
      resizeObserver = nil
    }
  }
}
