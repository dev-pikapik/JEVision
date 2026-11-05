import AppKit
import SwiftUI
import Testing

@testable import JEVision

struct WorkspacePanelsTests {
  @Test func panelsOnlyOccludeTheCanvasAndNeverMoveItsCoordinates() {
    let size = CGSize(width: 1440, height: 900)
    let left = WorkspaceInsets(leading: 356, top: 52).workingArea(in: size)
    let both = WorkspaceInsets(leading: 356, trailing: 396, top: 52).workingArea(in: size)
    #expect(left == CGRect(x: 356, y: 52, width: 1084, height: 848))
    #expect(both == CGRect(x: 356, y: 52, width: 688, height: 848))
    #expect(both.origin == left.origin)
    #expect(WorkspaceInsets(leading: 500, trailing: 500, top: 1000)
      .workingArea(in: size).height == 1)
  }

  @Test func resizingEitherPanelPreservesSpaceForTheCanvas() {
    #expect(WorkspacePanelWidth.clamp(800, minimum: 240, maximum: 600, available: 1440) == 600)
    #expect(WorkspacePanelWidth.clamp(100, minimum: 240, maximum: 600, available: 1440) == 240)
    #expect(WorkspacePanelWidth.clamp(600, minimum: 300, maximum: 600, available: 700) == 380)
    #expect(WorkspacePanelWidth.clamp(380, minimum: 300, maximum: 600, available: 560) == 240)
    #expect(WorkspacePanelWidth.clamp(380, minimum: 300, maximum: 600, available: 300) == 0)
    #expect(WorkspacePanelWidth.clamp(.nan, minimum: 240, maximum: 600, available: 1440) == 240)
  }

  @Test func eachDividerUsesTheStartingWidthAndItsOwnDirection() {
    #expect(WorkspacePanelWidth.resized(from: 340, by: 80, trailingEdge: true,
      minimum: 240, maximum: 600, available: 1440) == 420)
    #expect(WorkspacePanelWidth.resized(from: 380, by: 80, trailingEdge: false,
      minimum: 300, maximum: 600, available: 1440) == 300)
    #expect(WorkspacePanelWidth.resized(from: 380, by: -80, trailingEdge: false,
      minimum: 300, maximum: 600, available: 1440) == 460)
    #expect(WorkspacePanelWidth.resized(from: 340, by: 500, trailingEdge: true,
      minimum: 240, maximum: 600, available: 800) == 480)
  }

  @MainActor @Test func nativeSearchTypingCancelAndEscapeKeepTheBindingCurrent() {
    var query = ""
    let search = WorkspaceSearchField(text: Binding(get: { query }, set: { query = $0 }), prompt: "Find tests")
    let coordinator = search.makeCoordinator()
    let field = coordinator.makeField()
    field.stringValue = "checkout"
    coordinator.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: field))
    #expect(query == "checkout")

    // The native cancel button clears the field and sends its target/action.
    field.stringValue = ""
    coordinator.changed(field)
    #expect(query.isEmpty)

    field.stringValue = "cart"
    coordinator.changed(field)
    let editor = NSTextView()
    editor.string = "cart"
    #expect(coordinator.control(field, textView: editor, doCommandBy: #selector(NSResponder.cancelOperation(_:))))
    #expect(query.isEmpty)
    #expect(field.stringValue.isEmpty)
    #expect(editor.string.isEmpty)
    #expect(!coordinator.control(field, textView: editor, doCommandBy: #selector(NSResponder.cancelOperation(_:))))
  }

  @MainActor @Test func draggingThePanelHandleResizesSmoothlyInWindowCoordinates() throws {
    final class WidthBox { var value = 340.0 }
    let box = WidthBox()
    struct Harness: View {
      let box: WidthBox
      @State private var width = 340.0
      var body: some View {
        Color.clear
          .frame(width: width, height: 400)
          .overlay(alignment: .trailing) {
            WorkspacePanelResizer(
              width: Binding(get: { width }, set: { width = $0; box.value = $0 }),
              minimum: 240, maximum: 600, available: 1200, trailingEdge: true, label: Text("Projects"))
          }
      }
    }

    let host = NSHostingView(rootView: Harness(box: box))
    host.frame = CGRect(x: 0, y: 0, width: 800, height: 400)
    let window = NSWindow(
      contentRect: host.frame, styleMask: [.titled, .resizable], backing: .buffered, defer: false)
    window.contentView = host
    host.layoutSubtreeIfNeeded()

    func findHandle(in root: NSView) -> WorkspacePanelResizeHandle.HandleView? {
      if let handle = root as? WorkspacePanelResizeHandle.HandleView { return handle }
      for child in root.subviews {
        if let found = findHandle(in: child) { return found }
      }
      return nil
    }

    let handle = try #require(findHandle(in: host))
    handle.beginDrag(atWindowX: 340)
    handle.continueDrag(toWindowX: 400)
    host.layoutSubtreeIfNeeded()
    #expect(box.value == 400)
    handle.continueDrag(toWindowX: 460)
    host.layoutSubtreeIfNeeded()
    #expect(box.value == 460)
    handle.finishDrag()
  }

  @MainActor @Test func adaptiveGlassSidebarClipsScrollViewsToRoundedCornersWithoutScrollPockets() {
    let view = VStack(spacing: 0) {
      Text("Header").frame(height: 80)
      List {
        Section("Projects") {
          Text("Demo")
        }
      }
      .listStyle(.sidebar)
      .scrollContentBackground(.hidden)
      Text("Footer").frame(height: 30)
    }
    .frame(width: 340, height: 500)
    .adaptiveGlass(in: .rect(cornerRadius: 20))
    .padding(8)

    let host = NSHostingView(rootView: view)
    host.frame = CGRect(x: 0, y: 0, width: 356, height: 516)
    let window = NSWindow(
      contentRect: host.frame, styleMask: [.titled, .resizable], backing: .buffered, defer: false)
    window.contentView = host
    host.layoutSubtreeIfNeeded()
    window.displayIfNeeded()

    func hasRoundedClip(_ layer: CALayer) -> Bool {
      if layer.cornerRadius == 20 && layer.masksToBounds { return true }
      return (layer.sublayers ?? []).contains(where: hasRoundedClip)
    }
    func hasScrollPocket(_ view: NSView) -> Bool {
      if view.className.contains("NSScrollPocket") { return true }
      return view.subviews.contains(where: hasScrollPocket)
    }

    #expect(host.layer.map(hasRoundedClip) == true)
    #expect(!hasScrollPocket(host))
  }

  @MainActor @Test func workspaceSearchFieldUsesModernRoundedStyling() throws {
    var query = ""
    let search = WorkspaceSearchField(text: Binding(get: { query }, set: { query = $0 }), prompt: "Find tests")
    let host = NSHostingView(rootView: search.frame(width: 240, height: 28))
    host.frame = CGRect(x: 0, y: 0, width: 240, height: 28)
    let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
    window.contentView = host
    host.layoutSubtreeIfNeeded()

    func findSearchField(in view: NSView) -> NSSearchField? {
      if let sf = view as? NSSearchField { return sf }
      for sub in view.subviews {
        if let found = findSearchField(in: sub) { return found }
      }
      return nil
    }

    let field = try #require(findSearchField(in: host))
    #expect(field.wantsLayer)
    #expect(!field.isBordered)
    #expect(!field.isBezeled)
    #expect(!field.drawsBackground)
  }
}
