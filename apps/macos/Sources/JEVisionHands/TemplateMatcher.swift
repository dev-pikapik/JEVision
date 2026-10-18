import Foundation

/// An image as 8-bit RGBA pixels, row by row.
public struct Bitmap: Sendable {
  public let width: Int
  public let height: Int
  public let pixels: [UInt8]

  public init(width: Int, height: Int, pixels: [UInt8]) {
    precondition(pixels.count == width * height * 4)
    self.width = width
    self.height = height
    self.pixels = pixels
  }
}

/// Finds where a small image (a button, a field) appears in a screenshot.
///
/// A coarse search over shrunken grey copies picks the likeliest places, each is refined level
/// by level, and the final score is the runner's own similarity: the share of pixels whose
/// colour channels are all within `tolerance` of the template.
public enum TemplateMatcher {
  public struct Match: Equatable, Sendable {
    /// Top-left corner, in pixels of the screenshot.
    public let x: Int
    public let y: Int
    public let similarity: Double
  }

  /// Same as the runner's tolerance for similar images.
  static let tolerance = 16
  /// How many likely places the coarse search keeps.
  static let candidates = 12
  /// Rough number of pixel comparisons the coarse search may make.
  static let budget = 40_000_000.0

  /// The best place at least `minSimilarity` (0–1) alike, or `nil`. Gives up with `nil` once
  /// `isCancelled` says so.
  public static func find(
    _ template: Bitmap, in image: Bitmap, minSimilarity: Double,
    isCancelled: () -> Bool = { false }
  ) -> Match? {
    guard template.width > 0, template.height > 0,
      template.width <= image.width, template.height <= image.height
    else { return nil }

    // Shrink by halves until the coarse search fits the budget, keeping the template at least
    // 4 pixels on each side.
    let work = Double(image.width * image.height) * Double(template.width * template.height)
    var levels = 0
    while work / pow(16, Double(levels)) > budget,
      min(template.width, template.height) >> (levels + 1) >= 4
    {
      levels += 1
    }
    var images = [Grey(image)]
    var templates = [Grey(template)]
    for _ in 0..<levels {
      images.append(images[images.count - 1].halved())
      templates.append(templates[templates.count - 1].halved())
    }

    guard var places = coarse(templates[levels], in: images[levels], isCancelled: isCancelled) else { return nil }
    for level in stride(from: levels - 1, through: 1, by: -1) {
      places = places.map { place in
        let grown = (x: place.x * 2, y: place.y * 2)
        return best(around: grown, radius: 2, in: images[level], template: templates[level])
      }
      if isCancelled() { return nil }
    }

    var found: Match?
    for place in places {
      let origin = levels == 0 ? place : (x: place.x * 2, y: place.y * 2)
      for y in origin.y - 2...origin.y + 2 {
        for x in origin.x - 2...origin.x + 2
        where x >= 0 && y >= 0 && x + template.width <= image.width && y + template.height <= image.height {
          let floor = max(minSimilarity, found?.similarity ?? 0)
          if let s = similarity(template, image, x, y, atLeast: floor), s > (found?.similarity ?? -1), s >= minSimilarity {
            found = Match(x: x, y: y, similarity: s)
          }
        }
      }
      if isCancelled() { return nil }
    }
    return found
  }

  /// Share of template pixels that match at (`x`, `y`); `nil` as soon as it can't reach `atLeast`.
  static func similarity(_ t: Bitmap, _ image: Bitmap, _ x: Int, _ y: Int, atLeast: Double) -> Double? {
    let total = t.width * t.height
    let allowed = Int((1 - atLeast) * Double(total))
    var misses = 0
    return t.pixels.withUnsafeBufferPointer { tp in
      image.pixels.withUnsafeBufferPointer { ip in
        for row in 0..<t.height {
          var ti = row * t.width * 4
          var ii = ((y + row) * image.width + x) * 4
          for _ in 0..<t.width {
            if abs(Int(tp[ti]) - Int(ip[ii])) > tolerance || abs(Int(tp[ti + 1]) - Int(ip[ii + 1])) > tolerance
              || abs(Int(tp[ti + 2]) - Int(ip[ii + 2])) > tolerance
            {
              misses += 1
              if misses > allowed { return nil }
            }
            ti += 4
            ii += 4
          }
        }
        return Double(total - misses) / Double(total)
      }
    }
  }

  /// The `candidates` places with the smallest difference, kept apart from each other.
  private static func coarse(_ t: Grey, in image: Grey, isCancelled: () -> Bool) -> [(x: Int, y: Int)]? {
    var top: [(x: Int, y: Int, cost: Int)] = []
    let apart = max(1, min(t.width, t.height) / 2)
    for y in 0...(image.height - t.height) {
      if isCancelled() { return nil }
      for x in 0...(image.width - t.width) {
        let limit = top.count == candidates ? top[top.count - 1].cost : .max
        guard let cost = t.cost(in: image, x, y, limit: limit) else { continue }
        if let near = top.firstIndex(where: { abs($0.x - x) < apart && abs($0.y - y) < apart }) {
          guard cost < top[near].cost else { continue }
          top.remove(at: near)
        } else if top.count == candidates {
          top.removeLast()
        }
        let index = top.firstIndex { $0.cost > cost } ?? top.count
        top.insert((x, y, cost), at: index)
      }
    }
    return top.map { ($0.x, $0.y) }
  }

  private static func best(around p: (x: Int, y: Int), radius: Int, in image: Grey, template t: Grey) -> (x: Int, y: Int) {
    var found = p
    var lowest = Int.max
    for y in p.y - radius...p.y + radius {
      for x in p.x - radius...p.x + radius
      where x >= 0 && y >= 0 && x + t.width <= image.width && y + t.height <= image.height {
        if let cost = t.cost(in: image, x, y, limit: lowest), cost < lowest {
          lowest = cost
          found = (x, y)
        }
      }
    }
    return found
  }
}

/// Brightness only, for the coarse search.
private struct Grey {
  let width: Int
  let height: Int
  let values: [UInt8]

  init(width: Int, height: Int, values: [UInt8]) {
    self.width = width
    self.height = height
    self.values = values
  }

  init(_ b: Bitmap) {
    var values = [UInt8](repeating: 0, count: b.width * b.height)
    b.pixels.withUnsafeBufferPointer { p in
      for i in 0..<values.count {
        values[i] = UInt8((77 * Int(p[i * 4]) + 150 * Int(p[i * 4 + 1]) + 29 * Int(p[i * 4 + 2])) >> 8)
      }
    }
    self.init(width: b.width, height: b.height, values: values)
  }

  /// Half the size, each pixel the average of four.
  func halved() -> Grey {
    let w = width / 2, h = height / 2
    var out = [UInt8](repeating: 0, count: w * h)
    values.withUnsafeBufferPointer { v in
      for y in 0..<h {
        for x in 0..<w {
          let i = 2 * y * width + 2 * x
          out[y * w + x] = UInt8((Int(v[i]) + Int(v[i + 1]) + Int(v[i + width]) + Int(v[i + width + 1])) / 4)
        }
      }
    }
    return Grey(width: w, height: h, values: out)
  }

  /// Sum of squared differences with this template at (`x`, `y`); `nil` once it passes `limit`.
  func cost(in image: Grey, _ x: Int, _ y: Int, limit: Int) -> Int? {
    var sum = 0
    return values.withUnsafeBufferPointer { t in
      image.values.withUnsafeBufferPointer { v in
        for row in 0..<height {
          var ti = row * width
          var vi = (y + row) * image.width + x
          for _ in 0..<width {
            let d = Int(t[ti]) - Int(v[vi])
            sum += d * d
            ti += 1
            vi += 1
          }
          if sum >= limit { return nil }
        }
        return sum
      }
    }
  }
}
