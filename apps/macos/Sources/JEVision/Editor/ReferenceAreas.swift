import CoreGraphics
import Foundation
import JEVisionCore

/// The three kinds of areas a reference image can have.
enum AreaKind: CaseIterable, Hashable {
  /// The only part compared; without one, the whole image is.
  case compare
  /// Left out of the comparison: clocks, avatars, counters.
  case ignore
  /// Covered before the screenshot is sent to an AI model or saved in a report.
  case hide
}

/// One area of a reference: its kind and place in the list of that kind.
struct AreaID: Hashable {
  let kind: AreaKind
  let index: Int
}

extension Reference {
  /// Every area, the compare area first, then ignored and hidden ones in file order.
  var areaIDs: [AreaID] {
    (area == nil ? [] : [AreaID(kind: .compare, index: 0)])
      + ignore.indices.map { AreaID(kind: .ignore, index: $0) }
      + hide.indices.map { AreaID(kind: .hide, index: $0) }
  }

  subscript(area id: AreaID) -> Rect? {
    get {
      switch id.kind {
      case .compare: id.index == 0 ? area : nil
      case .ignore: ignore.indices.contains(id.index) ? ignore[id.index] : nil
      case .hide: hide.indices.contains(id.index) ? hide[id.index] : nil
      }
    }
    set {
      guard let newValue else { return remove(id) }
      switch id.kind {
      case .compare: if id.index == 0 { area = newValue }
      case .ignore: if ignore.indices.contains(id.index) { ignore[id.index] = newValue }
      case .hide: if hide.indices.contains(id.index) { hide[id.index] = newValue }
      }
    }
  }

  /// Adds an area and returns where it went. There is one compare area: a new one replaces it.
  @discardableResult
  mutating func add(_ kind: AreaKind, _ rect: Rect) -> AreaID {
    switch kind {
    case .compare:
      area = rect
      return AreaID(kind: .compare, index: 0)
    case .ignore:
      ignore.append(rect)
      return AreaID(kind: .ignore, index: ignore.count - 1)
    case .hide:
      hide.append(rect)
      return AreaID(kind: .hide, index: hide.count - 1)
    }
  }

  mutating func remove(_ id: AreaID) {
    switch id.kind {
    case .compare: if id.index == 0 { area = nil }
    case .ignore: if ignore.indices.contains(id.index) { ignore.remove(at: id.index) }
    case .hide: if hide.indices.contains(id.index) { hide.remove(at: id.index) }
    }
  }
}

/// A rectangle in whole pixels of the image, from its top-left corner.
struct PixelRect: Equatable {
  var x: Int
  var y: Int
  var width: Int
  var height: Int
}

/// Where an image of `pixels` size is drawn when fitted into `bounds`, and the mapping between
/// view points, pixels of the image and the fractions (0–1) the test file stores.
struct ImageFrame {
  let pixels: CGSize
  /// The image, fitted and centered in the view.
  let rect: CGRect

  init(pixels: CGSize, bounds: CGSize) {
    self.pixels = pixels
    guard pixels.width > 0, pixels.height > 0 else {
      rect = .zero
      return
    }
    let scale = min(bounds.width / pixels.width, bounds.height / pixels.height)
    let size = CGSize(width: pixels.width * scale, height: pixels.height * scale)
    rect = CGRect(
      x: (bounds.width - size.width) / 2, y: (bounds.height - size.height) / 2, width: size.width, height: size.height)
  }

  /// A point in the view as a fraction of the image, kept inside it.
  func fraction(_ point: CGPoint) -> CGPoint {
    guard rect.width > 0, rect.height > 0 else { return .zero }
    return CGPoint(
      x: min(max((point.x - rect.minX) / rect.width, 0), 1),
      y: min(max((point.y - rect.minY) / rect.height, 0), 1))
  }

  func view(_ r: Rect) -> CGRect {
    CGRect(
      x: rect.minX + r.x * rect.width, y: rect.minY + r.y * rect.height,
      width: r.width * rect.width, height: r.height * rect.height)
  }

  /// Fraction of the image a distance in view points covers.
  func fraction(_ size: CGSize) -> CGSize {
    guard rect.width > 0, rect.height > 0 else { return .zero }
    return CGSize(width: size.width / rect.width, height: size.height / rect.height)
  }

  func pixels(_ r: Rect) -> PixelRect {
    let left = Int((r.x * pixels.width).rounded()), top = Int((r.y * pixels.height).rounded())
    let right = Int(((r.x + r.width) * pixels.width).rounded())
    let bottom = Int(((r.y + r.height) * pixels.height).rounded())
    return PixelRect(x: left, y: top, width: right - left, height: bottom - top)
  }

  /// The fractions of `p`, kept inside the image and at least one pixel in size.
  func fraction(_ p: PixelRect) -> Rect {
    let w = max(Int(pixels.width), 1), h = max(Int(pixels.height), 1)
    let x = min(max(p.x, 0), w - 1), y = min(max(p.y, 0), h - 1)
    let width = min(max(p.width, 1), w - x), height = min(max(p.height, 1), h - y)
    return Rect(
      x: Double(x) / Double(w), y: Double(y) / Double(h),
      width: Double(width) / Double(w), height: Double(height) / Double(h))
  }

  /// `r` lined up with whole pixels, so the file says exactly which pixels it means.
  func snapped(_ r: Rect) -> Rect {
    fraction(pixels(r))
  }
}

/// Drawing, moving and resizing areas, all in fractions of the image.
enum AreaDrag {
  enum Corner: CaseIterable {
    case topLeft, topRight, bottomLeft, bottomRight

    func point(of r: Rect) -> CGPoint {
      switch self {
      case .topLeft: CGPoint(x: r.x, y: r.y)
      case .topRight: CGPoint(x: r.x + r.width, y: r.y)
      case .bottomLeft: CGPoint(x: r.x, y: r.y + r.height)
      case .bottomRight: CGPoint(x: r.x + r.width, y: r.y + r.height)
      }
    }

    var opposite: Corner {
      switch self {
      case .topLeft: .bottomRight
      case .topRight: .bottomLeft
      case .bottomLeft: .topRight
      case .bottomRight: .topLeft
      }
    }
  }

  /// The rectangle between two points, whichever way it was dragged.
  static func rect(from a: CGPoint, to b: CGPoint) -> Rect {
    Rect(x: min(a.x, b.x), y: min(a.y, b.y), width: abs(a.x - b.x), height: abs(a.y - b.y))
  }

  /// `r` moved by `offset`, stopping at the edges of the image.
  static func moved(_ r: Rect, by offset: CGSize) -> Rect {
    Rect(
      x: min(max(r.x + offset.width, 0), 1 - r.width), y: min(max(r.y + offset.height, 0), 1 - r.height),
      width: r.width, height: r.height)
  }

  /// `r` with `corner` dragged to `point`; the opposite corner stays put.
  static func resized(_ r: Rect, corner: Corner, to point: CGPoint) -> Rect {
    rect(from: corner.opposite.point(of: r), to: point)
  }
}
