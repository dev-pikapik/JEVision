import AppKit
import SwiftUI
import Testing

@testable import JEVision

@MainActor
struct CustomBlocksLayoutTests {
  static let source = "https://github.com/someone-else/blocks"

  /// A project with one block from the community whose address doesn't belong to its author.
  private func project() throws -> URL {
    let root = FileManager.default.temporaryDirectory.appending(path: "jevision-custom-blocks-\(UUID().uuidString)")
    let package = root.appending(path: "blocks/com.demo.links.jevblock")
    try FileManager.default.createDirectory(at: package, withIntermediateDirectories: true)
    let files = [
      package.appending(path: "block.json"): #"""
        {"format":1,"id":"com.demo.links","name":"Links demo","version":"1.0.0","author":"Acme","license":"MIT",
         "description":"Presses Command-S.","dependencies":[]}
        """#,
      package.appending(path: "flow.json"): #"""
        {"format":1,"name":"Links demo","app":"com.apple.TextEdit","start":"a","blocks":{"a":{"type":"pressKeys","keys":"cmd+s"}}}
        """#,
      root.appending(path: "jevision.blocks.json"): #"""
        {"format":1,"sources":{"com.demo.links":"\#(Self.source)"},"versions":{"com.demo.links":"1.0.0"}}
        """#,
    ]
    for (file, text) in files { try text.write(to: file, atomically: true, encoding: .utf8) }
    return root
  }

  private func views(in view: NSView) -> [NSView] { [view] + view.subviews.flatMap { views(in: $0) } }

  @Test func aBlockShowsItsNameFullAddressAndWarningWithoutScrolling() throws {
    let root = try project()
    defer { try? FileManager.default.removeItem(at: root) }
    let project = ProjectStore()
    project.open(root)
    #expect(project.library.blocks.contains { $0.package?.id == "com.demo.links" })

    // The sheet as it opens: as tall as it wants to be.
    let host = NSHostingView(rootView: CustomBlocksSheet().environment(project))
    let window = NSWindow(
      contentRect: NSRect(x: -10000, y: -10000, width: 560, height: host.fittingSize.height), styleMask: [.titled],
      backing: .buffered, defer: false)
    window.isReleasedWhenClosed = false
    window.contentView = host
    window.orderFront(nil)
    defer { window.close() }
    RunLoop.main.run(until: Date().addingTimeInterval(0.5))

    let all = views(in: host)
    let list = try #require(all.compactMap { $0 as? NSClipView }.first)
    let row = try #require(all.compactMap { $0 as? NSTableRowView }.filter { $0.isDescendant(of: list) }.max { $0.frame.height < $1.frame.height })
    let shown = list.convert(list.bounds, to: nil)
    let block = row.convert(row.bounds, to: nil)
    // The row holds the name, the address and the warning; all of it is in view without scrolling.
    #expect(shown.contains(block), "The block's row \(block) is in the list's view \(shown)")
    // The address can be selected, so it has a view of its own, which reads it out: as wide as the
    // whole address. That view's class differs between macOS versions; what it reads out doesn't.
    let inRow = all.filter { $0.isDescendant(of: row) }
    let address = try #require(
      inRow.first { $0.accessibilityLabel() == Self.source || $0.accessibilityValue() as? String == Self.source },
      "No view in the row reads out the address: \(inRow.map { String(describing: type(of: $0)) })")
    let font = NSFont.systemFont(ofSize: NSFont.preferredFont(forTextStyle: .caption1).pointSize)
    let needed = (Self.source as NSString).size(withAttributes: [.font: font]).width
    #expect(address.frame.width + 1 >= needed, "The address shows \(address.frame.width) of \(needed) points")
  }
}
