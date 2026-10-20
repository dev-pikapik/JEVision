import JEVisionCore
import Observation
import SwiftUI

/// How much of each block an open test on the board shows, by how large the blocks come out on
/// screen: the zoom times how much smaller than on the test's own canvas they are drawn.
enum BoardBlockDetail: Equatable {
  /// A tile in the block's color.
  case shape
  /// With its icon and name.
  case title
  /// With what it does, too.
  case summary

  init(scale: Double) {
    self = scale < 0.15 ? .shape : scale < 0.4 ? .title : .summary
  }
}

/// Where the blocks of an open test lie inside its card.
struct BoardBlockGeometry: Equatable {
  /// What the blocks take up, in the same points as the card.
  let area: CGRect
  /// How much smaller than on the test's own canvas the blocks are drawn.
  let fit: Double

  /// `nil` for a test with no blocks to show.
  init?(card: CGRect, layout: FlowLayout) {
    guard layout.width > 0, layout.height > 0 else { return nil }
    area = boardBlocksArea(card: Rect(card), width: layout.width, height: layout.height).cgRect
    fit = area.width / layout.width
  }

  func rect(of block: Rect) -> CGRect {
    CGRect(
      x: area.minX + block.x * fit, y: area.minY + block.y * fit, width: block.width * fit, height: block.height * fit)
  }

  /// The topmost block at a point in the card's points.
  func block(at point: CGPoint, in layout: FlowLayout) -> String? {
    guard area.contains(point) else { return nil }
    return layout.blocks.last { rect(of: $0.rect).contains(point) }?.id
  }
}

/// How the blocks of an open test did: in the run going on now, or else in the last saved one.
struct BoardBlockRun: Equatable {
  var marks: [String: BlockOutcome] = [:]
  /// The block running now.
  var current: String?

  /// "3 of 5 blocks passed, 1 failed", empty while no block has a mark.
  func text(total: Int) -> String {
    guard !marks.isEmpty else { return "" }
    let passed = marks.values.filter { $0 == .passed }.count
    let failed = marks.values.filter { $0 == .failed }.count
    var parts = [String(localized: "\(passed) of \(total) blocks passed")]
    if failed > 0 { parts.append(String(localized: "\(failed) failed")) }
    return parts.formatted(.list(type: .and, width: .narrow))
  }
}

/// A block of an open test, with the words it shows worked out once per change.
struct BoardBlockTile: Identifiable, Equatable {
  let id: String
  /// On the test's own canvas.
  let rect: CGRect
  let symbol: String
  let color: Color
  let title: String
  let summary: String
  /// Where it comes in reading order, from 1.
  let step: Int
  let issue: Severity?

  /// The blocks of `document` as tiles. `custom` are the library blocks, whose name and icon a block
  /// that runs one shows.
  @MainActor
  static func tiles(of document: FlowDocument, custom: [LibraryEntry]) -> [BoardBlockTile] {
    let folder = document.url.deletingLastPathComponent()
    let byFile = Dictionary(
      custom.compactMap { entry in entry.url.map { ($0.standardizedFileURL, entry) } }, uniquingKeysWith: { first, _ in first })
    return document.layout.blocks.compactMap { laid in
      guard let block = document.block(laid.id) else { return nil }
      let type = block.kind.typeName
      var entry: LibraryEntry?
      if case .runTest(let test, _, _, _) = block.kind, isBlockPath(path: test) {
        entry = byFile[folder.appending(path: test).standardizedFileURL]
      }
      let issues = document.issues(of: laid.id)
      return BoardBlockTile(
        id: laid.id, rect: laid.rect.cgRect, symbol: entry?.symbol ?? BlockInfo.symbol(type),
        color: BlockInfo.color(ofType: type), title: entry?.name ?? BlockInfo.title(type),
        summary: BlockSummary.text(block.kind, appName: { AppCatalog.shared.name(of: $0) }, testApp: document.flow?.app),
        step: document.step(of: laid.id),
        issue: issues.contains { $0.severity == .error } ? .error : issues.isEmpty ? nil : .warning)
    }
  }

  func runValue(_ mark: BlockOutcome?, isCurrent: Bool) -> String? {
    if isCurrent { return String(localized: "Running now") }
    switch mark {
    case .passed?: return String(localized: "Passed")
    case .failed?: return String(localized: "Failed")
    case .skipped?: return String(localized: "Skipped")
    case nil: return nil
    }
  }
}

/// The tests whose blocks show on the board: each is read once and kept while it stays open, and
/// the block whose fields are being looked at.
@MainActor
@Observable
final class BoardTestDocuments {
  /// A block of an open test whose fields are shown.
  struct Inspection: Identifiable, Equatable {
    let file: String
    let block: String
    var id: String { file + "#" + block }
  }

  var inspected: Inspection?
  @ObservationIgnored private var documents: [String: FlowDocument] = [:]
  @ObservationIgnored private var results: [String: (key: ResultsKey, last: LastRun?, run: BoardBlockRun)] = [:]

  private struct ResultsKey: Equatable {
    let newest: String?
    let count: Int
    let blocks: Int
  }

  /// The test `file` of the project in `root`, read when first asked for.
  func document(of file: String, in root: URL) -> FlowDocument {
    if let found = documents[file] { return found }
    let document = FlowDocument(url: root.appending(path: file), project: root)
    documents[file] = document
    return document
  }

  /// Reads the tests again, for changes made elsewhere.
  func reload() {
    for document in documents.values { document.reload() }
  }

  /// Lets go of the tests not in `files`.
  func keep(only files: Set<String>) {
    for file in documents.keys where !files.contains(file) {
      documents[file] = nil
      results[file] = nil
    }
    if let inspected, !files.contains(inspected.file) { dismiss() }
  }

  func inspect(_ file: String, block: String) {
    if let old = inspected, old.file != file { documents[old.file]?.selection = nil }
    documents[file]?.selection = block
    inspected = Inspection(file: file, block: block)
  }

  func dismiss() {
    if let inspected { documents[inspected.file]?.selection = nil }
    inspected = nil
  }

  /// How the blocks of `file` did in the newest saved run that ran them, worked out again only when the
  /// runs or the blocks change.
  func lastRun(of file: String, document: FlowDocument, runs: [SavedRun]) -> (last: LastRun?, run: BoardBlockRun) {
    guard let flow = document.flow else { return (nil, BoardBlockRun()) }
    let key = ResultsKey(newest: runs.first?.dir, count: runs.count, blocks: flow.blocks.count)
    if let cached = results[file], cached.key == key { return (cached.last, cached.run) }
    let last = LastRun(file: file, flow: flow, runs: runs, pinned: nil)
    let run = BoardBlockRun(marks: last.results.mapValues(\.outcome), current: nil)
    results[file] = (key, last, run)
    return (last, run)
  }
}

/// A test with its blocks shown inside its card: a title bar that says how the test stands and the
/// blocks, drawn in one go so a test of hundreds of blocks doesn't weigh on the board. Takes only
/// plain data, so it is drawn again only when what it shows changes, not on every move of the view.
struct BoardOpenTestView: View {
  @Environment(ProjectStore.self) private var project
  let file: String
  let name: String
  let status: BoardTestStatus
  let document: FlowDocument
  let documents: BoardTestDocuments
  let run: BoardBlockRun
  /// The card's size, in board points.
  let size: CGSize
  let detail: BoardBlockDetail
  let isPicked: Bool

  /// The most blocks VoiceOver is told of at once, so a test of hundreds doesn't swamp it.
  private static let spoken = 150
  private static let header = boardOpenHeader()

  var body: some View {
    let tiles = BoardBlockTile.tiles(of: document, custom: project.library.blocks)
    let layout = document.layout
    let isRunning = status.kind == .running
    let selected = documents.inspected?.file == file ? documents.inspected?.block : nil
    ZStack(alignment: .topLeading) {
      blocks(tiles, layout: layout, selected: selected)
      titleBar(tiles)
    }
    .frame(width: size.width, height: size.height, alignment: .topLeading)
    .background(.background, in: .rect(cornerRadius: 14))
    .overlay(
      RoundedRectangle(cornerRadius: 14)
        .strokeBorder(
          isPicked || isRunning ? AnyShapeStyle(.tint) : AnyShapeStyle(.separator),
          lineWidth: isPicked || isRunning ? 2 : 0.5)
    )
    .shadow(color: isRunning ? .accentColor.opacity(0.45) : .black.opacity(0.06), radius: isRunning ? 10 : 7, y: isRunning ? 0 : 4)
  }

  private func titleBar(_ tiles: [BoardBlockTile]) -> some View {
    let progress = run.current.flatMap { current in tiles.first { $0.id == current } }
      .map { String(localized: "Running block \($0.step) of \(tiles.count)") }
      ?? run.text(total: tiles.count)
    return HStack(spacing: 8) {
      Text(verbatim: name)
        .font(.headline)
        .lineLimit(1)
      Spacer(minLength: 8)
      HStack(spacing: 5) {
        if status.kind == .running {
          ProgressView().controlSize(.mini)
        } else {
          Image(systemName: status.symbol)
            .foregroundStyle(status.color)
        }
        Text(verbatim: progress.isEmpty ? status.text : progress)
          .lineLimit(1)
      }
      .font(.caption)
      .foregroundStyle(.secondary)
      Image(systemName: "chevron.down")
        .font(.caption.weight(.bold))
        .foregroundStyle(.secondary)
        .accessibilityHidden(true)
    }
    .padding(.horizontal, 12)
    .frame(width: size.width, height: Self.header)
    .accessibilityHidden(true)
  }

  private func blocks(_ tiles: [BoardBlockTile], layout: FlowLayout, selected: String?) -> some View {
    let detail = self.detail
    let run = self.run
    return Canvas { context, size in
      guard let geometry = BoardBlockGeometry(card: CGRect(origin: .zero, size: size), layout: layout) else { return }
      context.translateBy(x: geometry.area.minX, y: geometry.area.minY)
      context.scaleBy(x: geometry.fit, y: geometry.fit)
      let heavy = detail == .shape
      for edge in layout.edges where edge.points.count > 1 {
        let points = edge.points.map { CGPoint(x: $0.x, y: $0.y) }
        context.stroke(
          Edges.line(points), with: .style(.secondary), style: StrokeStyle(lineWidth: heavy ? 8 : 1.5, lineJoin: .round))
        if !heavy {
          context.fill(Edges.head(from: points[points.count - 2], to: points[points.count - 1]), with: .style(.secondary))
        }
      }
      var symbols: [String: GraphicsContext.ResolvedImage] = [:]
      for tile in tiles {
        Self.draw(
          tile, in: &context, detail: detail, mark: run.marks[tile.id], isCurrent: run.current == tile.id,
          isSelected: selected == tile.id, symbols: &symbols)
      }
    }
    .frame(width: size.width, height: size.height)
    .accessibilityChildren {
      ZStack(alignment: .topLeading) {
        if let geometry = BoardBlockGeometry(card: CGRect(origin: .zero, size: size), layout: document.layout) {
          ForEach(tiles.prefix(Self.spoken)) { tile in
            let rect = geometry.rect(of: Rect(tile.rect))
            Color.clear
              .frame(width: rect.width, height: rect.height)
              .accessibilityElement(children: .ignore)
              .accessibilityLabel(Text("Step \(tile.step) of \(tiles.count), \(tile.title)"))
              .accessibilityValue(Text(verbatim: spoken(tile, mark: run.marks[tile.id], isCurrent: run.current == tile.id)))
              .accessibilityHint(Text("Shows all the fields of the block"))
              .accessibilityAddTraits(selected == tile.id ? [.isButton, .isSelected] : .isButton)
              .accessibilityAction { documents.inspect(file, block: tile.id) }
              .offset(x: rect.minX, y: rect.minY)
          }
        }
      }
    }
  }

  private func spoken(_ tile: BoardBlockTile, mark: BlockOutcome?, isCurrent: Bool) -> String {
    ([tile.runValue(mark, isCurrent: isCurrent), tile.summary] + document.issues(of: tile.id).map(IssueText.line))
      .compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: ". ")
  }

  /// One block, on the test's own canvas scaled to the card. Little shows of it from afar: a tile in
  /// its color, thick lines and a large mark, so how it went reads at any size.
  private static func draw(
    _ tile: BoardBlockTile, in context: inout GraphicsContext, detail: BoardBlockDetail, mark: BlockOutcome?,
    isCurrent: Bool, isSelected: Bool, symbols: inout [String: GraphicsContext.ResolvedImage]
  ) {
    let rect = tile.rect
    let shape = Path(roundedRect: rect, cornerRadius: 14)
    let heavy = detail == .shape
    if heavy {
      context.fill(shape, with: .color(tile.color))
    } else {
      context.fill(shape, with: .style(.background))
    }
    let border: (style: GraphicsContext.Shading, width: Double) =
      isCurrent ? (.color(.orange), 2.5) : isSelected ? (.color(.accentColor), 2.5)
      : mark == .failed ? (.color(.red), 2) : (.style(.separator), 1)
    context.stroke(shape, with: border.style, lineWidth: border.width * (heavy ? 4 : 1))
    if detail != .shape {
      let icon = CGRect(x: rect.minX + 12, y: rect.midY - 17, width: 34, height: 34)
      context.fill(Path(roundedRect: icon, cornerRadius: 9), with: .color(tile.color))
      var symbol = symbols[tile.symbol] ?? context.resolve(Image(systemName: tile.symbol))
      symbol.shading = .color(.white)
      symbols[tile.symbol] = symbol
      let glyph = min(symbol.size.width, 17) / max(symbol.size.width, 1)
      let side = CGSize(width: symbol.size.width * glyph, height: symbol.size.height * glyph)
      context.draw(
        symbol, in: CGRect(x: icon.midX - side.width / 2, y: icon.midY - side.height / 2, width: side.width, height: side.height))
      let text = CGRect(x: icon.maxX + 10, y: rect.minY, width: max(0, rect.maxX - icon.maxX - 40), height: rect.height)
      context.drawLayer { layer in
        layer.clip(to: Path(text))
        let title = Text(verbatim: tile.title).font(.system(size: 13, weight: .semibold))
        if detail == .summary, !tile.summary.isEmpty {
          layer.draw(title, in: CGRect(x: text.minX, y: rect.midY - 19, width: text.width, height: 18))
          layer.draw(
            Text(verbatim: tile.summary).font(.system(size: 11)).foregroundStyle(.secondary),
            in: CGRect(x: text.minX, y: rect.midY, width: text.width, height: 20))
        } else {
          layer.draw(title, in: CGRect(x: text.minX, y: rect.midY - 9, width: text.width, height: 18))
        }
      }
    }
    let badge = heavy ? 56.0 : 18.0
    let box = CGRect(x: rect.maxX - badge * 0.7, y: rect.minY - badge * 0.3, width: badge, height: badge)
    if !isCurrent, let mark {
      let look: (symbol: String, color: Color) =
        switch mark {
        case .passed: ("checkmark.circle.fill", .green)
        case .failed: ("xmark.square.fill", .red)
        case .skipped: ("minus.circle.fill", .gray)
        }
      var image = symbols[look.symbol] ?? context.resolve(Image(systemName: look.symbol))
      image.shading = .color(look.color)
      symbols[look.symbol] = image
      context.fill(Path(ellipseIn: box.insetBy(dx: badge * 0.1, dy: badge * 0.1)), with: .color(.white))
      context.draw(image, in: box)
    } else if tile.issue != nil, !heavy {
      let symbol = tile.issue == .error ? "exclamationmark.octagon.fill" : "exclamationmark.triangle.fill"
      var image = symbols[symbol] ?? context.resolve(Image(systemName: symbol))
      image.shading = .color(tile.issue == .error ? .red : .orange)
      symbols[symbol] = image
      context.draw(image, in: box)
    }
  }
}
