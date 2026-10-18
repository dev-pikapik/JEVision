import CoreGraphics
import Foundation
import JEVisionCore
import Testing

@testable import JEVision

struct ImageFrameTests {
  /// A 200×100 px image in a 400×400 view: scaled twice, centered with 100 pt above and below.
  private let frame = ImageFrame(pixels: CGSize(width: 200, height: 100), bounds: CGSize(width: 400, height: 400))

  @Test func fitsAndCentersTheImage() {
    #expect(frame.rect == CGRect(x: 0, y: 100, width: 400, height: 200))
  }

  @Test func mapsViewPointsToFractionsAndBack() {
    #expect(frame.fraction(CGPoint(x: 100, y: 150)) == CGPoint(x: 0.25, y: 0.25))
    // Outside the image, the point stops at its edge.
    #expect(frame.fraction(CGPoint(x: -20, y: 390)) == CGPoint(x: 0, y: 1))
    #expect(frame.view(Rect(x: 0.5, y: 0.5, width: 0.25, height: 0.5)) == CGRect(x: 200, y: 200, width: 100, height: 100))
    #expect(frame.fraction(CGSize(width: 40, height: 20)) == CGSize(width: 0.1, height: 0.1))
  }

  @Test func mapsFractionsToWholePixels() {
    #expect(frame.pixels(Rect(x: 0.1, y: 0.2, width: 0.5, height: 0.3)) == PixelRect(x: 20, y: 20, width: 100, height: 30))
    #expect(frame.fraction(PixelRect(x: 20, y: 20, width: 100, height: 30)) == Rect(x: 0.1, y: 0.2, width: 0.5, height: 0.3))
    // Kept inside the image and at least a pixel in size.
    #expect(frame.fraction(PixelRect(x: -5, y: 90, width: 0, height: 50)) == Rect(x: 0, y: 0.9, width: 0.005, height: 0.1))
    #expect(frame.snapped(Rect(x: 0.1012, y: 0.2, width: 0.4977, height: 0.3)) == Rect(x: 0.1, y: 0.2, width: 0.5, height: 0.3))
  }

  @Test func emptyImageMapsToNothing() {
    let empty = ImageFrame(pixels: .zero, bounds: CGSize(width: 100, height: 100))
    #expect(empty.rect == .zero)
    #expect(empty.fraction(CGPoint(x: 50, y: 50)) == .zero)
  }
}

struct AreaDragTests {
  @Test func drawsInAnyDirection() {
    #expect(AreaDrag.rect(from: CGPoint(x: 0.5, y: 0.75), to: CGPoint(x: 0.25, y: 0.25)) == Rect(x: 0.25, y: 0.25, width: 0.25, height: 0.5))
  }

  @Test func movesWithinTheImage() {
    let r = Rect(x: 0.5, y: 0.5, width: 0.25, height: 0.25)
    #expect(AreaDrag.moved(r, by: CGSize(width: -0.25, height: 0.125)) == Rect(x: 0.25, y: 0.625, width: 0.25, height: 0.25))
    #expect(AreaDrag.moved(r, by: CGSize(width: 1, height: -1)) == Rect(x: 0.75, y: 0, width: 0.25, height: 0.25))
  }

  @Test func resizesByACornerKeepingTheOppositeOne() {
    let r = Rect(x: 0.25, y: 0.25, width: 0.5, height: 0.5)
    #expect(AreaDrag.resized(r, corner: .bottomRight, to: CGPoint(x: 1, y: 0.5)) == Rect(x: 0.25, y: 0.25, width: 0.75, height: 0.25))
    #expect(AreaDrag.resized(r, corner: .topLeft, to: CGPoint(x: 0, y: 0)) == Rect(x: 0, y: 0, width: 0.75, height: 0.75))
    // Dragged past the opposite corner, the area flips instead of turning inside out.
    #expect(AreaDrag.resized(r, corner: .topRight, to: CGPoint(x: 0, y: 1)) == Rect(x: 0, y: 0.75, width: 0.25, height: 0.25))
  }
}

struct ReferenceAreaModelTests {
  private let a = Rect(x: 0, y: 0, width: 0.5, height: 0.5)
  private let b = Rect(x: 0.5, y: 0.5, width: 0.5, height: 0.5)

  @Test func addsListsAndRemovesAreas() {
    var r = Reference(image: "refs/a.png", compare: .exact, area: nil, ignore: [], hide: [])
    #expect(r.add(.ignore, a) == AreaID(kind: .ignore, index: 0))
    #expect(r.add(.hide, b) == AreaID(kind: .hide, index: 0))
    #expect(r.add(.ignore, b) == AreaID(kind: .ignore, index: 1))
    #expect(r.add(.compare, a) == AreaID(kind: .compare, index: 0))
    #expect(r.areaIDs.map(\.kind) == [.compare, .ignore, .ignore, .hide])
    #expect(r.ignore == [a, b])

    // There is one compare area: a new one replaces it.
    r.add(.compare, b)
    #expect(r.area == b)

    r.remove(AreaID(kind: .ignore, index: 0))
    #expect(r.ignore == [b])
    r[area: AreaID(kind: .hide, index: 0)] = a
    #expect(r.hide == [a])
    r[area: AreaID(kind: .compare, index: 0)] = nil
    #expect(r.area == nil)
    #expect(r[area: AreaID(kind: .ignore, index: 5)] == nil)
    r.remove(AreaID(kind: .hide, index: 5))
    #expect(r.hide == [a])
  }
}

struct ReferenceFileTests {
  @Test func namesTheDarkPictureLikeTheRunner() {
    #expect(referenceDarkVariant(image: "refs/inbox.png") == "refs/inbox.dark.png")
    #expect(referenceDarkVariant(image: "a.b/inbox") == "a.b/inbox.dark")
    #expect(referenceDarkVariant(image: ".hidden") == ".hidden.dark")
  }

  @Test func picksAFreeNameForANewPicture() throws {
    let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: folder.appending(path: "refs"), withIntermediateDirectories: true)
    #expect(ReferenceFile.newPath(for: "check", in: folder) == "refs/check.png")
    try Data().write(to: folder.appending(path: "refs/check.png"))
    #expect(ReferenceFile.newPath(for: "check", in: folder) == "refs/check-2.png")
    #expect(ReferenceFile.newPath(for: "a/b c", in: folder) == "refs/a-b-c.png")
  }
}

@MainActor
struct ReferenceUndoTests {
  @Test func endingAStepKeepsChangesToOneFieldApartForUndo() throws {
    let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let url = folder.appending(path: "t.flow.json")
    try """
    {"format": 1, "name": "T", "app": "com.apple.TextEdit", "start": "c", "blocks": {
      "c": {"type": "check", "reference": {"image": "refs/c.png"}}}}
    """.write(to: url, atomically: true, encoding: .utf8)
    let document = FlowDocument(url: url)
    let undo = UndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo

    func edit(_ change: (inout Reference) -> Void) {
      document.update("c", field: "reference") { kind in
        guard case .check(var reference, let next) = kind else { return }
        change(&reference)
        kind = .check(reference: reference, next: next)
      }
    }
    func ignored() -> [Rect] {
      if case .check(let reference, _) = document.block("c")?.kind { return reference.ignore }
      return []
    }

    let area = Rect(x: 0.1, y: 0.1, width: 0.2, height: 0.2)
    for _ in 0..<2 {
      undo.beginUndoGrouping()
      document.endUndoStep()
      edit { $0.add(.ignore, area) }
      undo.endUndoGrouping()
    }
    #expect(ignored().count == 2)
    #expect(try String(contentsOf: url, encoding: .utf8).contains("\"ignore\""))

    undo.undo()
    #expect(ignored().count == 1)
  }
}
