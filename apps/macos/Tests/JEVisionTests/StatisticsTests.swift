import CoreGraphics
import JEVisionCore
import SwiftUI
import Testing

@testable import JEVision

@MainActor
struct TrendBarsTests {
  /// The columns of the chart's picture that show `color`, told apart by which channel leads.
  private func columns(withoutColor: Bool, showing color: (UInt8, UInt8, UInt8) -> Bool) throws -> Set<Int> {
    let trend = [TrendPoint(startMs: 1_760_000_000_000, passed: 4, passedOnRetry: 2, failed: 3, averageMs: 1_000)]
    let chart = TrendBars(trend: trend, unit: .day, withoutColor: withoutColor)
      .chartLegend(.hidden)
      .frame(width: 240, height: 180)
    let image = try #require(ImageRenderer(content: chart).cgImage)
    let (width, height) = (image.width, image.height)
    var pixels = [UInt8](repeating: 0, count: width * height * 4)
    let context = try #require(
      CGContext(
        data: &pixels, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
        space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
    context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
    var found = Set<Int>()
    for y in 0..<height {
      for x in 0..<width {
        let i = (y * width + x) * 4
        if pixels[i + 3] > 200, color(pixels[i], pixels[i + 1], pixels[i + 2]) { found.insert(x) }
      }
    }
    return found
  }

  @Test func withoutColorEachResultStandsBesideTheOthersInsteadOfOnThem() throws {
    let green = { (r: UInt8, g: UInt8, b: UInt8) in g > 150 && r < 120 && b < 150 }
    let red = { (r: UInt8, g: UInt8, b: UInt8) in r > 200 && g < 110 && b < 110 }
    let stacked = (try columns(withoutColor: false, showing: green), try columns(withoutColor: false, showing: red))
    #expect(!stacked.0.isEmpty && !stacked.1.isEmpty)
    #expect(!stacked.0.isDisjoint(with: stacked.1), "stacked: passed and failed share a column")
    let beside = (try columns(withoutColor: true, showing: green), try columns(withoutColor: true, showing: red))
    #expect(!beside.0.isEmpty && !beside.1.isEmpty)
    #expect(beside.0.isDisjoint(with: beside.1), "without color: passed and failed stand apart")
  }
}

@MainActor
struct FailingBlockPlaceTests {
  @Test func aFailingBlockIsNamedAsTheCanvasNamesIt() throws {
    let root = FileManager.default.temporaryDirectory.appending(path: "jevision-place-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    let flow = #"{"format":1,"name":"Total","app":"com.apple.calculator","start":"total-display","blocks":{"total-display":{"type":"check","reference":{"image":"refs/total.png"}}}}"#
    try flow.write(to: root.appending(path: "total.flow.json"), atomically: true, encoding: .utf8)
    let project = ProjectStore()
    project.open(root)

    let place = { (shown: String?) in
      RunText.place(block: "total-display", blockType: "check", test: "total.flow.json", shownTest: shown, in: project)
    }
    // Its type and what it does, as on the canvas; the id is for tools, not for people.
    #expect(place("total.flow.json") == "Check · Screen matches “total”")
    #expect(place("suite.flow.json") == "Check · Screen matches “total” · in total")
    // A block whose test can't be read is still named by its type.
    #expect(
      RunText.place(block: "gone", blockType: "click", test: "gone.flow.json", shownTest: "gone.flow.json", in: project) == "Click")
  }
}
