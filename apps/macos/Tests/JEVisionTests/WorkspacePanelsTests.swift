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
}
