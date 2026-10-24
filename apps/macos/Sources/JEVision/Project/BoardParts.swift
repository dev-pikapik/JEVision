import AppKit
import ImageIO
import JEVisionCore
import SwiftUI
import UniformTypeIdentifiers

/// The colors a board node or arrow can have: JSON Canvas presets `1` to `6` or `#rrggbb`.
enum BoardColor {
  static let presets: [(id: String, name: String, color: Color)] = [
    ("1", String(localized: "Red"), .red), ("2", String(localized: "Orange"), .orange),
    ("3", String(localized: "Yellow"), .yellow), ("4", String(localized: "Green"), .green),
    ("5", String(localized: "Cyan"), .cyan), ("6", String(localized: "Purple"), .purple),
  ]

  static func color(_ value: String) -> Color? {
    if let preset = presets.first(where: { $0.id == value }) { return preset.color }
    guard value.count == 7, value.hasPrefix("#"), let rgb = Int(value.dropFirst(), radix: 16) else { return nil }
    return Color(
      red: Double(rgb >> 16 & 0xFF) / 255, green: Double(rgb >> 8 & 0xFF) / 255, blue: Double(rgb & 0xFF) / 255)
  }
}

/// How a node looks from afar, on the minimap and when the board is zoomed far out.
struct BoardGlance: Equatable {
  enum Shape { case frame, card, note, picture }
  let shape: Shape
  let title: String
  /// The node's own color; a test's card takes the color of how it stands.
  let color: Color?
  /// How a test, or the tests in a frame, stand, as a shape beside the color.
  var badge: BoardTestStatus?
}

/// Square grid lines behind the board: finer lines fade out as the board zooms out, so the grid
/// never turns into a grey wash, and every sixth line is stronger.
struct BoardGrid: View {
  let viewport: BoardViewport

  var body: some View {
    Canvas { context, size in
      let scale = viewport.scale
      var step = FlowCanvas.grid
      while step * scale < 4 { step *= 6 }
      // From the finest lines shown to ones as far apart as the view is wide; each line is drawn
      // once, as strong as the coarsest grid it belongs to.
      while step * scale < max(size.width, size.height) * 6 {
        let spacing = step * scale
        let strength = min(max((spacing - 4) / 60, 0), 1)
        var lines = Path()
        let first = (-viewport.offset.x / spacing).rounded(.down)
        for i in stride(from: first, through: first + size.width / spacing + 1, by: 1) where i.remainder(dividingBy: 6) != 0 {
          let x = i * spacing + viewport.offset.x
          lines.move(to: CGPoint(x: x, y: 0))
          lines.addLine(to: CGPoint(x: x, y: size.height))
        }
        let top = (-viewport.offset.y / spacing).rounded(.down)
        for i in stride(from: top, through: top + size.height / spacing + 1, by: 1) where i.remainder(dividingBy: 6) != 0 {
          let y = i * spacing + viewport.offset.y
          lines.move(to: CGPoint(x: 0, y: y))
          lines.addLine(to: CGPoint(x: size.width, y: y))
        }
        context.stroke(lines, with: .color(.primary.opacity(0.03 + 0.09 * strength)), lineWidth: 1)
        step *= 6
      }
    }
    .accessibilityHidden(true)
  }
}

/// The board from afar: every node in view as a plain shape in one drawing, so hundreds of tests
/// pan and zoom smoothly.
struct BoardOverview: View {
  struct Item {
    let id: String
    /// Where it is on the board.
    let rect: CGRect
    let glance: BoardGlance
  }

  let items: [Item]
  let picked: Set<String>
  let viewport: BoardViewport

  var body: some View {
    Canvas { context, _ in
      let scale = viewport.scale
      var badges: [String: GraphicsContext.ResolvedImage] = [:]
      for item in items {
        let glance = item.glance
        let rect = CGRect(
          x: item.rect.minX * scale + viewport.offset.x, y: item.rect.minY * scale + viewport.offset.y,
          width: item.rect.width * scale, height: item.rect.height * scale)
        guard rect.width >= 3, rect.height >= 3 else {
          // Too small to tell shapes apart: a speck of its color.
          context.fill(Path(rect), with: .color(glance.color ?? .secondary.opacity(0.6)))
          continue
        }
        let corner = min(14 * scale, rect.height / 3)
        let shape = Path(roundedRect: rect, cornerRadius: corner)
        switch glance.shape {
        case .frame:
          let tint = glance.color ?? .secondary
          context.fill(shape, with: .color(tint.opacity(0.08)))
          context.stroke(shape, with: .color(tint.opacity(0.35)), lineWidth: 1)
          let header = min(rect.height, boardFrameHeader() * scale)
          if header >= 10 {
            let size = min(13, header * 0.55)
            let title = Text(verbatim: glance.title).font(.system(size: size, weight: .semibold)).foregroundStyle(.secondary)
            let room = glance.badge == nil ? 16.0 : 16 + header
            context.draw(
              title, in: CGRect(x: rect.minX + 8, y: rect.minY + (header - size) / 2 - 2, width: max(0, rect.width - room), height: header))
            Self.badge(glance, in: CGRect(x: rect.maxX - header, y: rect.minY, width: header, height: header), context: context, cache: &badges)
          }
        case .card, .note, .picture:
          context.fill(shape, with: .style(.background))
          if glance.shape == .note, let tint = glance.color { context.fill(shape, with: .color(tint.opacity(0.15))) }
          if glance.shape == .picture {
            context.fill(shape, with: .color(.secondary.opacity(0.12)))
          }
          context.stroke(shape, with: .style(.separator), lineWidth: 0.5)
          if glance.shape == .card, let tint = glance.color {
            let bar = CGRect(x: rect.minX, y: rect.minY, width: max(2, 5 * scale), height: rect.height)
            context.fill(Path(roundedRect: bar, cornerRadius: min(2, bar.width / 2)), with: .color(tint))
          }
          let side = min(rect.height - 4, max(16, 28 * scale))
          var room = 0.0
          if glance.badge != nil, side >= 12 {
            Self.badge(glance, in: CGRect(x: rect.maxX - side - 4, y: rect.minY + 2, width: side, height: side), context: context, cache: &badges)
            room = side + 4
          }
          if scale >= 0.2 {
            context.draw(
              Text(verbatim: glance.title).font(.subheadline),
              in: rect.insetBy(dx: max(4, 8 * scale), dy: max(2, 6 * scale)).inset(trailing: room))
          }
        }
        if picked.contains(item.id) {
          context.stroke(shape, with: .color(.accentColor), lineWidth: 2)
        }
      }
    }
  }

  /// The shape of how it stands in the corner of `box`, when the box is big enough to tell.
  private static func badge(
    _ glance: BoardGlance, in box: CGRect, context: GraphicsContext, cache: inout [String: GraphicsContext.ResolvedImage]
  ) {
    guard let status = glance.badge, box.height >= 12 else { return }
    let key = "\(status.symbol)|\(status.kind)"
    var image = cache[key] ?? context.resolve(Image(systemName: status.symbol))
    image.shading = .color(status.color)
    cache[key] = image
    let size = min(box.height - 4, 16)
    context.draw(image, in: CGRect(x: box.midX - size / 2, y: box.midY - size / 2, width: size, height: size))
  }
}

private extension CGRect {
  func inset(trailing: Double) -> CGRect {
    CGRect(x: minX, y: minY, width: max(0, width - trailing), height: height)
  }
}

/// The whole board small in a corner, with the part in view outlined. A click or a drag moves
/// the view there.
struct BoardMinimap: View {
  let layout: BoardLayout
  /// How each node looks, by its position in `layout.nodes`.
  let glances: [BoardGlance]
  /// Changes whenever `layout` or `glances` do, so the nodes are drawn again only then.
  let revision: Int
  /// The part of the board in view, in board points.
  let visible: CGRect
  /// Moves the view so this board point is in the middle.
  let jump: (CGPoint) -> Void

  static let size = CGSize(width: 200, height: 132)

  var body: some View {
    let world = self.world
    let scale = min(Self.size.width / world.width, Self.size.height / world.height)
    let origin = CGPoint(
      x: (Self.size.width - world.width * scale) / 2 - world.minX * scale,
      y: (Self.size.height - world.height * scale) / 2 - world.minY * scale)
    ZStack {
      Nodes(layout: layout, glances: glances, revision: revision, world: world).equatable()
      Canvas { context, _ in
        let view = CGRect(
          x: visible.minX * scale + origin.x, y: visible.minY * scale + origin.y, width: visible.width * scale,
          height: visible.height * scale)
        context.fill(Path(roundedRect: view, cornerRadius: 3), with: .color(.accentColor.opacity(0.12)))
        context.stroke(Path(roundedRect: view, cornerRadius: 3), with: .color(.accentColor), lineWidth: 1.5)
      }
    }
    .frame(width: Self.size.width, height: Self.size.height)
    .contentShape(.rect)
    .gesture(
      DragGesture(minimumDistance: 0).onChanged { value in
        jump(CGPoint(x: (value.location.x - origin.x) / scale, y: (value.location.y - origin.y) / scale))
      }
    )
    .accessibilityElement()
    .accessibilityLabel(Text("Minimap"))
    .accessibilityValue(Text("\(layout.nodes.count) items"))
  }

  /// The board and the part in view, with room around them.
  private var world: CGRect {
    let content = layout.bounds?.cgRect ?? visible
    let union = content.union(visible)
    return union.insetBy(dx: -max(40, union.width * 0.05), dy: -max(40, union.height * 0.05))
  }

  /// The nodes alone, which stay as they are while the view moves about the board.
  private struct Nodes: View, Equatable {
    let layout: BoardLayout
    let glances: [BoardGlance]
    let revision: Int
    let world: CGRect

    nonisolated static func == (lhs: Self, rhs: Self) -> Bool {
      lhs.revision == rhs.revision && lhs.world == rhs.world
    }

    var body: some View {
      Canvas { context, _ in
        let scale = min(BoardMinimap.size.width / world.width, BoardMinimap.size.height / world.height)
        let origin = CGPoint(
          x: (BoardMinimap.size.width - world.width * scale) / 2 - world.minX * scale,
          y: (BoardMinimap.size.height - world.height * scale) / 2 - world.minY * scale)
        for (position, node) in layout.nodes.enumerated() where position < glances.count {
          let glance = glances[position]
          let r = node.rect.cgRect
          let rect = CGRect(x: r.minX * scale + origin.x, y: r.minY * scale + origin.y, width: r.width * scale, height: r.height * scale)
          switch glance.shape {
          case .frame:
            context.fill(Path(roundedRect: rect, cornerRadius: 2), with: .color((glance.color ?? .secondary).opacity(0.15)))
          case .card, .note, .picture:
            context.fill(Path(rect), with: .color(glance.color ?? .secondary.opacity(0.6)))
          }
        }
      }
    }
  }
}

/// A frame on a board: a tinted area with its title bar; folded, only the bar shows. The bar says
/// how the tests inside it stand.
struct BoardFrameView: View {
  let label: String
  let color: Color?
  let isFolded: Bool
  let isPicked: Bool
  /// Tests inside it, counted for the folded bar.
  let tests: Int
  /// How the tests inside it stand; `nil` for a frame with none.
  var rollup: BoardRollup?

  var body: some View {
    let tint = color ?? .secondary
    VStack(alignment: .leading, spacing: 0) {
      HStack(spacing: 8) {
        Image(systemName: isFolded ? "chevron.right" : "chevron.down")
          .font(.caption.weight(.bold))
          .foregroundStyle(.secondary)
          .frame(width: 16)
        Text(verbatim: label)
          .font(.headline)
          .lineLimit(1)
        if isFolded, tests > 0 {
          Text("\(tests) tests")
            .font(.callout)
            .foregroundStyle(.secondary)
            .lineLimit(1)
        }
        Spacer(minLength: 0)
        if let rollup, let lead = rollup.lead {
          HStack(spacing: 5) {
            Image(systemName: lead.symbol)
              .foregroundStyle(lead.color)
            Text(verbatim: rollup.text)
              .lineLimit(1)
          }
          .font(.callout)
          .foregroundStyle(.secondary)
        }
      }
      .padding(.horizontal, 14)
      .frame(height: boardFrameHeader())
      Spacer(minLength: 0)
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    .background(tint.opacity(color == nil ? 0.07 : 0.1), in: .rect(cornerRadius: 20))
    .overlay(
      RoundedRectangle(cornerRadius: 20)
        .strokeBorder(isPicked ? AnyShapeStyle(.tint) : AnyShapeStyle(tint.opacity(0.35)), lineWidth: isPicked ? 2 : 1)
    )
  }
}

/// A note on a board, in Markdown: headings, lists, check boxes and inline styles.
struct BoardNoteView: View {
  let text: String
  let color: Color?
  let isPicked: Bool

  var body: some View {
    VStack(alignment: .leading, spacing: 4) {
      if text.isEmpty {
        Text("Double-click to write a note").foregroundStyle(.secondary)
      } else {
        MarkdownText(text: text)
      }
    }
    .font(.body)
    .padding(14)
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    .clipped()
    .background(.background, in: .rect(cornerRadius: 14))
    .background((color ?? .clear).opacity(0.18), in: .rect(cornerRadius: 14))
    .overlay(
      RoundedRectangle(cornerRadius: 14)
        .strokeBorder(isPicked ? AnyShapeStyle(.tint) : AnyShapeStyle(color ?? Color(nsColor: .separatorColor)), lineWidth: isPicked ? 2 : 1)
    )
    .shadow(color: .black.opacity(0.06), radius: 7, y: 4)
  }
}

/// A board node that points elsewhere: another board, a web page or a file of the project.
struct BoardLinkCard: View {
  let symbol: String
  let title: String
  let detail: String
  let color: Color?
  let isPicked: Bool
  let isMissing: Bool

  var body: some View {
    HStack(spacing: 10) {
      Image(systemName: symbol)
        .font(.title3)
        .foregroundStyle(color ?? .accentColor)
        .frame(width: 24)
      VStack(alignment: .leading, spacing: 2) {
        Text(verbatim: title)
          .font(.headline)
          .lineLimit(1)
        Text(verbatim: detail)
          .font(.caption)
          .foregroundStyle(.secondary)
          .lineLimit(1)
          .truncationMode(.middle)
      }
      Spacer(minLength: 0)
    }
    .padding(.horizontal, 12)
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .background(.background, in: .rect(cornerRadius: 14))
    .background((color ?? .clear).opacity(0.12), in: .rect(cornerRadius: 14))
    .overlay(
      RoundedRectangle(cornerRadius: 14)
        .strokeBorder(isPicked ? AnyShapeStyle(.tint) : AnyShapeStyle(.separator), lineWidth: isPicked ? 2 : 0.5)
    )
    .shadow(color: .black.opacity(0.06), radius: 7, y: 4)
    .opacity(isMissing ? 0.6 : 1)
  }
}

/// A picture on a board, kept in the project: it fills its node with its proportions kept.
struct BoardImageView: View {
  /// The picture's file, relative to the project.
  let file: String
  let root: URL?
  let color: Color?
  let isPicked: Bool
  @State private var image: NSImage?
  @State private var isMissing = false

  var body: some View {
    ZStack {
      if let image {
        Image(nsImage: image)
          .resizable()
          .scaledToFit()
      } else if isMissing {
        VStack(spacing: 6) {
          Image(systemName: "photo.badge.exclamationmark")
            .font(.title)
          Text("Picture not found")
            .font(.callout)
        }
        .foregroundStyle(.secondary)
      } else {
        ProgressView().controlSize(.small)
      }
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .background((color ?? .secondary).opacity(0.1), in: .rect(cornerRadius: 12))
    .clipShape(.rect(cornerRadius: 12))
    .overlay(
      RoundedRectangle(cornerRadius: 12)
        .strokeBorder(isPicked ? AnyShapeStyle(.tint) : AnyShapeStyle(color ?? Color(nsColor: .separatorColor)), lineWidth: isPicked ? 2 : 0.5)
    )
    .shadow(color: .black.opacity(0.06), radius: 7, y: 4)
    .task(id: file) {
      guard let root else { return }
      let url = root.appending(path: file)
      isMissing = false
      if let found = await BoardImages.picture(at: url) {
        image = found
      } else {
        image = nil
        isMissing = true
      }
    }
  }
}

/// Pictures read from the project for the board, as large as the board can show them, kept
/// while the board is in use so scrolling back to one doesn't read it again.
enum BoardImages {
  /// The most pixels a picture is read at, per side.
  static let limit = 2048
  nonisolated(unsafe) private static let cache = NSCache<NSString, NSImage>()

  static func picture(at url: URL) async -> NSImage? {
    let modified = (try? url.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
    let key = "\(url.path)|\(modified?.timeIntervalSince1970 ?? 0)" as NSString
    if let kept = cache.object(forKey: key) { return kept }
    let read = await Task.detached(priority: .userInitiated) { thumbnail(at: url) }.value
    if let read { cache.setObject(read, forKey: key) }
    return read
  }

  /// The picture a drop or a paste holds: the file it names, or else the image itself as a PNG.
  @MainActor static func read(_ provider: NSItemProvider) async -> (data: Data, name: String)? {
    if provider.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) {
      let url: URL? = await withCheckedContinuation { continuation in
        _ = provider.loadObject(ofClass: URL.self) { url, _ in continuation.resume(returning: url) }
      }
      if let url, url.isFileURL,
        let data = await Task.detached(priority: .userInitiated, operation: { try? Data(contentsOf: url) }).value
      {
        return (data, url.lastPathComponent)
      }
    }
    guard let type = provider.registeredContentTypes.first(where: { $0.conforms(to: .image) }),
      let suffix = type.preferredFilenameExtension
    else { return nil }
    let data: Data? = await withCheckedContinuation { continuation in
      _ = provider.loadDataRepresentation(forTypeIdentifier: type.identifier) { data, _ in
        continuation.resume(returning: data)
      }
    }
    return data.map { ($0, "Image." + suffix) }
  }

  nonisolated private static func thumbnail(at url: URL) -> NSImage? {
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
    let options: [CFString: Any] = [
      kCGImageSourceCreateThumbnailFromImageAlways: true, kCGImageSourceCreateThumbnailWithTransform: true,
      kCGImageSourceThumbnailMaxPixelSize: limit,
    ]
    guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
    return NSImage(cgImage: image, size: NSSize(width: image.width, height: image.height))
  }
}

/// Arrows drawn between board nodes: curves that leave and enter the nodes' sides, with heads
/// where the file asks for them, and the dashed arrows of the tests one test runs.
struct BoardArrows: View {
  struct Arrow {
    let id: String
    let from: CGPoint
    let fromSide: String
    let to: CGPoint
    let toSide: String
    let fromHead: Bool
    let toHead: Bool
    let label: String
    let color: Color?
  }

  let arrows: [Arrow]
  let uses: [ProjectMap.Link]
  let picked: String?
  let viewport: BoardViewport
  /// Dashed arrows outside this part of the board (in board points) aren't drawn.
  var area: CGRect?

  var body: some View {
    Canvas { context, _ in
      let transform = CGAffineTransform(translationX: viewport.offset.x, y: viewport.offset.y)
        .scaledBy(x: viewport.scale, y: viewport.scale)
      UsesArrows.draw(uses, in: context, transform: transform, area: area)
      for arrow in arrows {
        let curve = Self.curve(arrow, scale: viewport.scale).applying(transform)
        let color = arrow.id == picked ? Color.accentColor : arrow.color ?? .secondary
        context.stroke(curve.path, with: .color(color), lineWidth: arrow.id == picked ? 2.5 : 1.75)
        if arrow.toHead { context.fill(Self.head(at: curve.end, from: curve.endControl), with: .color(color)) }
        if arrow.fromHead { context.fill(Self.head(at: curve.start, from: curve.startControl), with: .color(color)) }
        if !arrow.label.isEmpty {
          let text = context.resolve(Text(verbatim: arrow.label).font(.caption).foregroundStyle(.secondary))
          let size = text.measure(in: CGSize(width: 240, height: 40))
          let middle = curve.point(at: 0.5)
          let box = CGRect(x: middle.x - size.width / 2 - 6, y: middle.y - size.height / 2 - 3, width: size.width + 12, height: size.height + 6)
          context.fill(Path(roundedRect: box, cornerRadius: 6), with: .style(.background))
          context.draw(text, in: box.insetBy(dx: 6, dy: 3))
        }
      }
    }
    .accessibilityHidden(true)
  }

  struct Curve {
    var start, startControl, endControl, end: CGPoint

    var path: Path {
      Path { path in
        path.move(to: start)
        path.addCurve(to: end, control1: startControl, control2: endControl)
      }
    }

    func point(at t: Double) -> CGPoint {
      let u = 1 - t
      let a = u * u * u, b = 3 * u * u * t, c = 3 * u * t * t, d = t * t * t
      return CGPoint(
        x: a * start.x + b * startControl.x + c * endControl.x + d * end.x,
        y: a * start.y + b * startControl.y + c * endControl.y + d * end.y)
    }

    func applying(_ transform: CGAffineTransform) -> Curve {
      Curve(
        start: start.applying(transform), startControl: startControl.applying(transform),
        endControl: endControl.applying(transform), end: end.applying(transform))
    }
  }

  /// The curve of `arrow` in board points: it leaves and arrives square to the sides.
  static func curve(_ arrow: Arrow, scale: Double) -> Curve {
    let reach = max(40, min(160, hypot(arrow.to.x - arrow.from.x, arrow.to.y - arrow.from.y) / 2.5))
    func out(_ point: CGPoint, _ side: String) -> CGPoint {
      switch side {
      case "top": CGPoint(x: point.x, y: point.y - reach)
      case "bottom": CGPoint(x: point.x, y: point.y + reach)
      case "left": CGPoint(x: point.x - reach, y: point.y)
      default: CGPoint(x: point.x + reach, y: point.y)
      }
    }
    return Curve(start: arrow.from, startControl: out(arrow.from, arrow.fromSide), endControl: out(arrow.to, arrow.toSide), end: arrow.to)
  }

  private static func head(at end: CGPoint, from control: CGPoint) -> Path {
    let angle = atan2(end.y - control.y, end.x - control.x)
    var head = Path()
    head.move(to: end)
    head.addLine(to: CGPoint(x: end.x - 10 * cos(angle - 0.42), y: end.y - 10 * sin(angle - 0.42)))
    head.addLine(to: CGPoint(x: end.x - 10 * cos(angle + 0.42), y: end.y - 10 * sin(angle + 0.42)))
    head.closeSubpath()
    return head
  }
}

/// Takes the trackpad and the mouse wheel over the board, and right clicks: two fingers pan,
/// a pinch or ⌘ with the wheel zooms, a two-finger double tap zooms in or out to fit. Every
/// other click goes through to the views under it; text being written keeps its own scrolling.
struct BoardInput: NSViewRepresentable {
  /// Moves the board by a distance in view points.
  let pan: (CGSize) -> Void
  /// Zooms by a factor around a point of the view.
  let zoom: (Double, CGPoint) -> Void
  let smartZoom: (CGPoint) -> Void
  /// The menu for a right click at a point of the view.
  let menu: (CGPoint) -> NSMenu?
  var capturesSecondaryClicks = true

  func makeNSView(context: Context) -> InputView { InputView() }

  func updateNSView(_ view: InputView, context: Context) {
    view.owner = self
  }

  final class InputView: NSView {
    var owner: BoardInput?
    private var monitor: Any?

    override var isFlipped: Bool { true }

    override func viewDidMoveToWindow() {
      super.viewDidMoveToWindow()
      if let monitor { NSEvent.removeMonitor(monitor) }
      monitor = nil
      guard window != nil else { return }
      monitor = NSEvent.addLocalMonitorForEvents(matching: [.scrollWheel, .magnify, .smartMagnify]) { [weak self] event in
        guard let self, let owner, event.window === window, window?.attachedSheet == nil else { return event }
        let point = convert(event.locationInWindow, from: nil)
        guard bounds.contains(point), !isOverText(event) else { return event }
        switch event.type {
        case .scrollWheel:
          if event.modifierFlags.contains(.command) {
            let delta = event.hasPreciseScrollingDeltas ? event.scrollingDeltaY : event.scrollingDeltaY * 8
            owner.zoom(exp(delta * 0.01), point)
          } else {
            let factor = event.hasPreciseScrollingDeltas ? 1.0 : 10
            owner.pan(CGSize(width: event.scrollingDeltaX * factor, height: event.scrollingDeltaY * factor))
          }
        case .magnify: owner.zoom(1 + event.magnification, point)
        default: owner.smartZoom(point)
        }
        return nil
      }
    }

    /// The pointer is over text being written, which scrolls by itself.
    private func isOverText(_ event: NSEvent) -> Bool {
      guard let content = window?.contentView, let superview = content.superview else { return false }
      var view = content.hitTest(superview.convert(event.locationInWindow, from: nil))
      while let current = view, current !== self {
        if current is NSTextView { return true }
        view = current.superview
      }
      return false
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
      guard owner?.capturesSecondaryClicks == true,
        let event = NSApp.currentEvent, Self.isSecondary(event) else { return nil }
      return super.hitTest(point)
    }

    override func menu(for event: NSEvent) -> NSMenu? {
      owner?.menu(convert(event.locationInWindow, from: nil))
    }

    override func mouseDown(with event: NSEvent) {
      guard Self.isSecondary(event), let menu = menu(for: event) else { return }
      NSMenu.popUpContextMenu(menu, with: event, for: self)
    }

    private static func isSecondary(_ event: NSEvent) -> Bool {
      switch event.type {
      case .rightMouseDown, .rightMouseUp: true
      case .leftMouseDown, .leftMouseUp: event.modifierFlags.contains(.control)
      default: false
      }
    }
  }
}

/// A menu item that runs a closure.
final class BoardMenuItem: NSMenuItem {
  private let handler: () -> Void

  init(_ title: String, symbol: String? = nil, image: NSImage? = nil, checked: Bool = false, enabled: Bool = true, handler: @escaping () -> Void) {
    self.handler = handler
    super.init(title: title, action: #selector(run), keyEquivalent: "")
    target = self
    self.image = image ?? symbol.flatMap { NSImage(systemSymbolName: $0, accessibilityDescription: nil) }
    state = checked ? .on : .off
    isEnabled = enabled
  }

  required init(coder: NSCoder) { fatalError("init(coder:) is not used") }

  @objc private func run() { handler() }

  /// A submenu item titled `title` holding `items`.
  static func submenu(_ title: String, symbol: String, items: [NSMenuItem]) -> NSMenuItem {
    let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
    item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)
    let menu = NSMenu(title: title)
    menu.autoenablesItems = false
    items.forEach(menu.addItem)
    item.submenu = menu
    return item
  }

  /// A round swatch of `color` for a color menu.
  static func swatch(_ color: Color?) -> NSImage {
    NSImage(size: NSSize(width: 12, height: 12), flipped: false) { rect in
      if let color {
        NSColor(color).setFill()
        NSBezierPath(ovalIn: rect.insetBy(dx: 0.5, dy: 0.5)).fill()
      } else {
        NSColor.secondaryLabelColor.setStroke()
        NSBezierPath(ovalIn: rect.insetBy(dx: 1, dy: 1)).stroke()
      }
      return true
    }
  }
}

extension Rect {
  init(_ rect: CGRect) {
    self.init(x: rect.minX, y: rect.minY, width: rect.width, height: rect.height)
  }
}
