import Foundation
import Testing

@testable import JEVision

struct LayoutTests {
  private static func size(_ width: CGFloat) -> CGSize { CGSize(width: width, height: 20) }

  @Test func wrappedViewsFillARowThenStartTheNext() {
    let frames = WrapLayout.frames(width: 100, spacing: 10, sizes: [Self.size(40), Self.size(40), Self.size(40)])
    #expect(frames.map(\.origin) == [CGPoint(x: 0, y: 0), CGPoint(x: 50, y: 0), CGPoint(x: 0, y: 30)])
  }

  @Test func aViewWiderThanTheRowGetsARowOfItsOwnAtTheRowsWidth() {
    let frames = WrapLayout.frames(width: 100, spacing: 10, sizes: [Self.size(40), Self.size(160), Self.size(30)])
    #expect(frames[1] == CGRect(x: 0, y: 30, width: 100, height: 20))
    #expect(frames[2].origin == CGPoint(x: 0, y: 60))
  }

  @Test func nothingToWrapTakesNoRoom() {
    #expect(WrapLayout.frames(width: 100, spacing: 10, sizes: []).isEmpty)
  }

  @Test func buttonsSitBesideALabelOnlyWhileItKeepsItsRoom() {
    let row = ActionRowLayout(labelWidth: 200)
    #expect(row.sitsBeside(actions: 180, width: 400))
    #expect(!row.sitsBeside(actions: 190, width: 400))
  }
}
