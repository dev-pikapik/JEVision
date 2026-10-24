import CoreGraphics
import JEVisionCore
import SwiftUI
import Testing

@testable import JEVision

@MainActor
struct FlowViewportTests {
  @Test func gridKeepsItsOriginLinesAtEveryZoomAndPixelDensity() throws {
    for zoom in [0.25, 1, 1.5, 2] {
      for dpi in [1.0, 2.0] {
        let origin = CGPoint(x: 96.4, y: 80.4)
        let renderer = ImageRenderer(content: BoardGrid(viewport: BoardViewport(offset: origin, scale: zoom))
          .environment(\.displayScale, dpi).frame(width: 320, height: 240))
        renderer.scale = dpi
        let image = try #require(renderer.cgImage)
        var pixels = [UInt8](repeating: 0, count: image.width * image.height * 4)
        let context = try #require(CGContext(data: &pixels, width: image.width, height: image.height,
          bitsPerComponent: 8, bytesPerRow: image.width * 4, space: CGColorSpaceCreateDeviceRGB(),
          bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        let column = Int((origin.x * dpi).rounded(.down))
        let ink = (0..<image.height).filter { pixels[($0 * image.width + column) * 4 + 3] > 0 }.count
        #expect(Double(ink) > Double(image.height) * 0.9, "The world origin must not be skipped by every grid level")
        let row = Int((origin.y * dpi).rounded(.down))
        let counts: [Int] = [row, image.height - row - 1].map { y in
          (0..<image.width).filter { pixels[(y * image.width + $0) * 4 + 3] > 0 }.count
        }
        let horizontal = max(counts[0], counts[1])
        #expect(Double(horizontal) > Double(image.width) * 0.9)
      }
    }
  }

  @Test func connectorStemsEndAtTheArrowBaseAndHandleShortOrRepeatedSegments() {
    let arrow = Edges.connector([.zero, CGPoint(x: 50, y: 0), CGPoint(x: 50, y: 100)])
    #expect(arrow.base == CGPoint(x: 50, y: 92))
    #expect(arrow.stem.currentPoint == arrow.base)
    #expect(arrow.stem.boundingRect.maxY == 92)
    #expect(arrow.head.boundingRect.maxY == 100)
    let short = Edges.connector([.zero, .zero, CGPoint(x: 1, y: 0), CGPoint(x: 1, y: 0)])
    #expect(short.base == CGPoint(x: 0.25, y: 0))
    #expect(short.stem.currentPoint == short.base)
    #expect(Edges.connector([.zero, .zero]).head.isEmpty)
  }

  @Test func emptyFrameInteriorsAllowWorkspaceDrags() {
    let rect = CGRect(x: 0, y: 0, width: 400, height: 300)
    let frame = CanvasItemHitShape(isFrame: true).path(in: rect)
    #expect(frame.contains(CGPoint(x: 100, y: 20), eoFill: true))
    #expect(frame.contains(CGPoint(x: 2, y: 100), eoFill: true))
    #expect(!frame.contains(CGPoint(x: 100, y: 100), eoFill: true))
    #expect(CanvasItemHitShape(isFrame: false).path(in: rect).contains(CGPoint(x: 100, y: 100)))
  }

  @Test func panelsAndViewportResizeDoNotMoveTheDrawing() {
    let camera = FlowViewport(offset: CGPoint(x: 430, y: 70), scale: 1.5)
    let block = CGPoint(x: -200, y: -20)
    let before = camera.view(block)
    let wide = camera.visible(in: CGSize(width: 1000, height: 700))
    let narrow = camera.visible(in: CGSize(width: 680, height: 700))
    #expect(wide.origin == narrow.origin)
    #expect(wide.width > narrow.width)
    #expect(camera.view(block) == before)
    #expect(camera.world(before) == block)
  }

  @Test func fitAndMinimapIncludeNegativeItemsAndTheVisibleArea() throws {
    let layout = FlowLayout(
      width: 0, height: 0,
      blocks: [LaidBlock(id: "a", rect: Rect(x: -900, y: -600, width: 208, height: 64))],
      frames: [], edges: [], slots: [], ports: [])
    let bounds = FlowViewport.bounds(layout: layout, canvas: nil)
    #expect(bounds.contains(layout.blocks[0].rect.cgRect))
    var camera = FlowViewport()
    let size = CGSize(width: 800, height: 600)
    camera.scale = Zoom.fit(bounds.size, in: size)
    camera.center(on: CGPoint(x: bounds.midX, y: bounds.midY), in: size)
    let visible = camera.visible(in: size)
    #expect(visible.contains(bounds))
    let projection = FlowMapProjection(content: bounds, visible: visible, size: FlowMinimap.size)
    #expect(projection.world.contains(bounds))
    #expect(projection.world.contains(visible))
    let point = CGPoint(x: -850, y: -550)
    let roundTrip = projection.worldPoint(projection.point(point))
    #expect(abs(roundTrip.x - point.x) < 0.0001)
    #expect(abs(roundTrip.y - point.y) < 0.0001)
    #expect(CGRect(origin: .zero, size: FlowMinimap.size).contains(projection.rect(visible)))
    let card = try #require(
      BoardBlockGeometry(card: CGRect(x: 0, y: 0, width: 400, height: 300), layout: layout))
    let rect = card.rect(of: layout.blocks[0].rect)
    #expect(card.block(at: CGPoint(x: rect.midX, y: rect.midY), in: layout) == "a")
  }

  @Test func arrowsAcrossNegativeCoordinatesRenderAtDoubleZoom() throws {
    let edge = LaidEdge(
      from: "a", edge: "next", to: "b", points: [Point(x: -240, y: 0), Point(x: 80, y: 0)])
    let layout = FlowLayout(
      width: 0, height: 0, blocks: [], frames: [], edges: [edge], slots: [], ports: [])
    let view = Edges(
      layout: layout, drawingBounds: CGRect(x: -300, y: -100, width: 600, height: 200),
      renderScale: 2
    )
    .offset(x: 300, y: 100)
    .frame(width: 600, height: 200, alignment: .topLeading)
    let image = try #require(ImageRenderer(content: view).cgImage)
    var pixels = [UInt8](repeating: 0, count: image.width * image.height * 4)
    let context = try #require(
      CGContext(
        data: &pixels, width: image.width, height: image.height,
        bitsPerComponent: 8, bytesPerRow: image.width * 4, space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
    context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
    func hasInk(in columns: Range<Int>) -> Bool {
      (0..<image.height).contains { y in
        columns.contains { x in pixels[(y * image.width + x) * 4 + 3] > 0 }
      }
    }
    #expect(hasInk(in: 70..<100), "Connector drawn left of the world origin")
    #expect(hasInk(in: 330..<360), "Connector drawn right of the world origin")
  }


  @Test func doubledCanvasTextMatchesNativeSizeRendering() throws {
    let section = CanvasSection(id: "frame", title: "Sharp text", rect: Rect(x: 0, y: 0, width: 208, height: 64))
    func frame() -> some View {
      FrameView(section: section, isPicked: true, isEditing: false, title: .constant(section.title), endEditing: {}, scale: 2)
    }
    let native = frame().frame(width: 416, height: 128)
    let transformed = frame().canvasElementSize(CGSize(width: 208, height: 64), scale: 2)
      .scaleEffect(2, anchor: .topLeading)
      .frame(width: 416, height: 128, alignment: .topLeading)
    let expected = try #require(ImageRenderer(content: native).cgImage)
    let actual = try #require(ImageRenderer(content: transformed).cgImage)
    #expect(actual.width == expected.width && actual.height == expected.height)
    let expectedPixels = try #require(expected.dataProvider?.data) as Data
    let actualPixels = try #require(actual.dataProvider?.data) as Data
    #expect(actualPixels == expectedPixels, "Canvas transforms must preserve the native glyph pixels")
  }

}
