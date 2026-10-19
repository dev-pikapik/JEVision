import JEVisionCore
import SwiftUI

/// How the tests of a board hang together, apart from drawing them.
enum ProjectMap {
  struct Group: Identifiable {
    /// The folder, relative to the project; empty for tests at the top.
    let id: String
    let title: String
    let tests: [ProjectItem]
  }

  struct Link: Equatable {
    let from: CGRect
    let to: CGRect
    /// Where the arrow runs, around the cards and folder titles between the two.
    let points: [CGPoint]
    /// `to` must pass before `from` runs; otherwise `from` runs `to`.
    var isDependency = false
  }

  /// The key of a folder title among the cards' frames: arrows go around it too.
  static func labelKey(_ folder: String) -> String { "folder:" + folder }

  /// A frame per folder that holds tests itself, in sidebar order: the project's own tests first.
  static func groups(_ items: [ProjectItem], rootTitle: String) -> [Group] {
    func walk(_ items: [ProjectItem], folder: String, title: String) -> [Group] {
      let tests = items.filter { $0.kind == .test }
      let own = tests.isEmpty ? [] : [Group(id: folder, title: title, tests: tests)]
      return own + items.filter { $0.kind == .folder }.flatMap { walk($0.children, folder: $0.id, title: $0.id) }
    }
    return walk(items, folder: "", title: rootTitle)
  }

  /// The newest run of each test, by file relative to the project.
  static func latestRuns(_ runs: [SavedRun]) -> [String: SavedRun] {
    var latest: [String: (date: Date, run: SavedRun)] = [:]
    for run in runs {
      let date = run.startedAt
      if (latest[run.report.file]?.date ?? .distantPast) <= date { latest[run.report.file] = (date, run) }
    }
    return latest.mapValues(\.run)
  }

  /// The tests that run each test, by file.
  static func usedBy(_ map: [String: MapTest]) -> [String: [String]] {
    var usedBy: [String: [String]] = [:]
    for test in map.values.sorted(by: { $0.file < $1.file }) {
      for used in test.uses where used != test.file { usedBy[used, default: []].append(test.file) }
    }
    return usedBy
  }

  /// An arrow from each test to each test it runs and each test it depends on, between the
  /// cards' frames. `bounds` also holds the frames of the folder titles.
  static func links(_ map: [String: MapTest], bounds: [String: CGRect]) -> [Link] {
    let keys = bounds.keys.sorted()
    return map.keys.sorted().flatMap { file -> [Link] in
      guard let from = bounds[file], let test = map[file] else { return [] }
      let targets = test.uses.map { ($0, false) } + test.dependsOn.map { ($0, true) }
      return targets.compactMap { target, isDependency in
        guard target != file, let to = bounds[target] else { return nil }
        let others = keys.filter { $0 != file && $0 != target }.compactMap { bounds[$0].map(rect) }
        let points = mapLinkRoute(from: rect(from), to: rect(to), others: others).map { CGPoint(x: $0.x, y: $0.y) }
        return Link(from: from, to: to, points: points, isDependency: isDependency)
      }
    }
  }

  private static func rect(_ r: CGRect) -> Rect {
    Rect(x: r.minX, y: r.minY, width: r.width, height: r.height)
  }
}

struct DashedLine: Shape {
  func path(in rect: CGRect) -> Path {
    Path { path in
      path.move(to: CGPoint(x: rect.minX, y: rect.midY))
      path.addLine(to: CGPoint(x: rect.maxX, y: rect.midY))
    }
  }
}

/// Arrows from a card to the cards of the tests it runs, dashed, and of the tests it depends on,
/// solid in the accent colour; in straight runs with rounded turns that go around the other cards.
enum UsesArrows {
  /// Draws `links`, laid out in board points, where `transform` puts the board.
  static func draw(_ links: [ProjectMap.Link], in context: GraphicsContext, transform: CGAffineTransform) {
    for link in links {
      let points = link.points.map { $0.applying(transform) }
      guard points.count > 1, let end = points.last else { continue }
      var line = Path()
      line.move(to: points[0])
      for i in 1..<points.count - 1 {
        let (before, corner, after) = (points[i - 1], points[i], points[i + 1])
        // Short runs get a tighter turn, so two turns never overlap.
        let room = min(hypot(corner.x - before.x, corner.y - before.y), hypot(after.x - corner.x, after.y - corner.y))
        line.addArc(tangent1End: corner, tangent2End: after, radius: min(8, room / 2))
      }
      line.addLine(to: end)
      let shading: GraphicsContext.Shading = link.isDependency ? .color(.accentColor) : .color(.secondary)
      context.stroke(line, with: shading, style: StrokeStyle(lineWidth: 1.5, dash: link.isDependency ? [] : [5, 4]))
      // The head points the way the last run arrives.
      let last = points[points.count - 2]
      let angle = atan2(end.y - last.y, end.x - last.x)
      var head = Path()
      head.move(to: end)
      head.addLine(to: CGPoint(x: end.x - 8 * cos(angle - 0.45), y: end.y - 8 * sin(angle - 0.45)))
      head.addLine(to: CGPoint(x: end.x - 8 * cos(angle + 0.45), y: end.y - 8 * sin(angle + 0.45)))
      head.closeSubpath()
      context.fill(head, with: shading)
    }
  }
}

/// A test on a board: its name, how it last ran or that it runs now, quarantine and flakiness,
/// and a bar per block. It fills the card's place on the board.
struct TestCard: View {
  /// The test's name; its file when the test is gone.
  let name: String
  let info: MapTest?
  let last: SavedRun?
  let quarantine: Quarantined?
  let flaky: FlakyEvidence?
  /// Names of the tests it runs, and of the tests that run it.
  let uses: [String]
  let usedBy: [String]
  /// Names of the tests that must pass before it runs.
  let needs: [String]
  let isRunning: Bool
  /// The test file is no longer in the project.
  let isMissing: Bool
  let isPicked: Bool

  var body: some View {
    VStack(alignment: .leading, spacing: 4) {
      Text(verbatim: name)
        .font(.headline)
        .lineLimit(1)
      HStack(spacing: 5) {
        if isRunning {
          ProgressView().controlSize(.mini)
        } else {
          Image(systemName: state.symbol)
            .foregroundStyle(state.color)
        }
        Text(state.text)
          .lineLimit(1)
      }
      .font(.caption)
      .foregroundStyle(.secondary)
      if quarantine != nil || (flaky != nil && last?.status != .flaky) {
        HStack(spacing: 8) {
          if let quarantine {
            Label {
              Text(QuarantineText.describe(quarantine)).lineLimit(1)
            } icon: {
              Image(systemName: RunStatus.quarantined.symbol).foregroundStyle(RunStatus.quarantined.color)
            }
          }
          if let flaky, last?.status != .flaky {
            Label {
              Text("Flaky").lineLimit(1)
            } icon: {
              Image(systemName: RunStatus.flaky.symbol).foregroundStyle(RunStatus.flaky.color)
            }
            .help(Text(RunHistory.flakyEvidence(flaky)))
          }
        }
        .font(.caption)
        .foregroundStyle(.secondary)
      }
      if !needs.isEmpty {
        Label {
          Text("After \(needs.formatted(.list(type: .and)))").lineLimit(2)
        } icon: {
          Image(systemName: "arrow.turn.down.right").foregroundStyle(.tint)
        }
        .font(.caption)
        .foregroundStyle(.secondary)
      }
      Spacer(minLength: 0)
      if let blocks = info?.blocks, blocks > 0 {
        // A short bar per block, up to ten: how big the test is at a glance.
        HStack(spacing: 3) {
          ForEach(0..<min(Int(blocks), 10), id: \.self) { _ in
            RoundedRectangle(cornerRadius: 2)
              .fill(.quaternary)
              .frame(width: 14, height: 5)
          }
        }
      }
    }
    .padding(12)
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    .background(.background, in: .rect(cornerRadius: 14))
    .overlay(
      RoundedRectangle(cornerRadius: 14)
        .strokeBorder(isPicked ? AnyShapeStyle(.tint) : AnyShapeStyle(.separator), lineWidth: isPicked ? 2 : 0.5)
    )
    .shadow(color: .black.opacity(0.06), radius: 7, y: 4)
    .opacity(isMissing ? 0.6 : 1)
  }

  var state: (symbol: String, color: Color, text: String) {
    if isMissing {
      return ("questionmark.folder", .secondary, String(localized: "Not in the project any more"))
    }
    if isRunning { return ("play.fill", .accentColor, String(localized: "Running…")) }
    if let info, !info.readable {
      return ("exclamationmark.triangle.fill", .red, String(localized: "Can’t be read"))
    }
    if let info, info.errors > 0 {
      return ("exclamationmark.octagon.fill", .red, String(localized: "Can’t run: fix the problems in the test"))
    }
    guard let last else { return ("circle.dashed", .secondary, String(localized: "Not run yet")) }
    let when = last.startedAt.formatted(.relative(presentation: .named, unitsStyle: .abbreviated))
    return (last.status.symbol, last.status.color, "\(last.status.title) · \(when)")
  }

  /// Everything the card says, for VoiceOver.
  var accessibilityValue: String {
    var parts = [state.text]
    if let quarantine { parts.append(QuarantineText.describe(quarantine)) }
    if let flaky { parts.append(RunHistory.flakyEvidence(flaky)) }
    if !uses.isEmpty { parts.append(String(localized: "Runs \(uses.formatted(.list(type: .and)))")) }
    if !usedBy.isEmpty { parts.append(String(localized: "Run by \(usedBy.formatted(.list(type: .and)))")) }
    if !needs.isEmpty { parts.append(String(localized: "Runs after \(needs.formatted(.list(type: .and))) pass")) }
    return parts.joined(separator: ". ")
  }
}
