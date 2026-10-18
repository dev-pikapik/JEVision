import JEVisionCore
import SwiftUI

/// Every test of the project as a card in its folder's frame, with how it last ran and dashed
/// arrows to the tests it runs. A click opens the test.
struct ProjectMapView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(RunCenter.self) private var runs
  @Environment(RecordCenter.self) private var records
  /// What the core read from each test, by file relative to the project.
  @State private var map: [String: MapTest] = [:]

  var body: some View {
    let groups = ProjectMap.groups(project.items, rootTitle: project.name)
    let latest = ProjectMap.latestRuns(project.runs)
    let names = Dictionary(groups.flatMap(\.tests).map { ($0.id, $0.name) }, uniquingKeysWith: { a, _ in a })
    let usedBy = ProjectMap.usedBy(map)
    ScrollView([.horizontal, .vertical]) {
      HStack(alignment: .top, spacing: 28) {
        ForEach(groups) { group in
          VStack(alignment: .leading, spacing: 12) {
            Label(group.title, systemImage: "folder")
              .font(.subheadline.weight(.semibold))
              .foregroundStyle(.secondary)
              .lineLimit(1)
              .accessibilityAddTraits(.isHeader)
              .anchorPreference(key: CardBounds.self, value: .bounds) { [ProjectMap.labelKey(group.id): $0] }
            ForEach(group.tests) { test in
              let info = map[test.id]
              MapCard(
                test: test, info: info, last: latest[test.id], quarantine: project.quarantine(of: test.id),
                flaky: project.flaky[test.id],
                uses: (info?.uses ?? []).compactMap { names[$0] },
                usedBy: (usedBy[test.id] ?? []).compactMap { names[$0] }
              ) { project.selection = test.id }
              .anchorPreference(key: CardBounds.self, value: .bounds) { [test.id: $0] }
            }
          }
          .padding(16)
          .frame(width: MapCard.width + 32, alignment: .leading)
          .background(.quaternary.opacity(0.35), in: .rect(cornerRadius: 20))
          .overlay(RoundedRectangle(cornerRadius: 20).strokeBorder(.separator))
          .accessibilityElement(children: .contain)
        }
      }
      .padding(32)
      .backgroundPreferenceValue(CardBounds.self) { bounds in
        GeometryReader { proxy in
          UsesArrows(links: ProjectMap.links(map, bounds: bounds.mapValues { proxy[$0] }))
        }
        .accessibilityHidden(true)
      }
    }
    .safeAreaInset(edge: .top, spacing: 0) { header(count: groups.reduce(0) { $0 + $1.tests.count }) }
    .task(id: project.items) { await load() }
  }

  private func header(count: Int) -> some View {
    HStack(spacing: 16) {
      VStack(alignment: .leading, spacing: 2) {
        Text("Project Map")
          .font(.title2.weight(.semibold))
        Text("\(count) tests")
          .font(.callout)
          .foregroundStyle(.secondary)
      }
      Spacer(minLength: 12)
      HStack(spacing: 6) {
        DashedLine()
          .stroke(.secondary, style: StrokeStyle(lineWidth: 1.5, dash: [5, 4]))
          .frame(width: 28, height: 2)
        Text("runs another test")
      }
      .font(.callout)
      .foregroundStyle(.secondary)
      .accessibilityHidden(true)
      Button("Run All", systemImage: "play.fill") {
        guard let root = project.rootURL else { return }
        runs.runAll(project.items.flatMap(\.tests).map(\.url), in: root, title: String(localized: "All tests"))
      }
      .buttonStyle(.glassProminent)
      .disabled(runs.isRunning || records.isRecording || project.rootURL == nil)
    }
    .padding(.horizontal, 24)
    .padding(.vertical, 14)
  }

  private func load() async {
    guard let root = project.rootURL else { return }
    let path = root.path(percentEncoded: false)
    let files = project.items.flatMap(\.tests).map(\.id)
    let tests = await Task.detached(priority: .userInitiated) { projectMap(project: path, tests: files) }.value
    guard !Task.isCancelled else { return }
    map = Dictionary(tests.map { ($0.file, $0) }, uniquingKeysWith: { a, _ in a })
  }
}

/// How the map lays out the project, apart from drawing it.
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
    var latest: [String: SavedRun] = [:]
    for run in runs where (latest[run.report.file]?.startedAt ?? .distantPast) <= run.startedAt {
      latest[run.report.file] = run
    }
    return latest
  }

  /// The tests that run each test, by file.
  static func usedBy(_ map: [String: MapTest]) -> [String: [String]] {
    var usedBy: [String: [String]] = [:]
    for test in map.values.sorted(by: { $0.file < $1.file }) {
      for used in test.uses where used != test.file { usedBy[used, default: []].append(test.file) }
    }
    return usedBy
  }

  /// An arrow from each test to each test it runs, between the cards' frames. `bounds` also
  /// holds the frames of the folder titles.
  static func links(_ map: [String: MapTest], bounds: [String: CGRect]) -> [Link] {
    let keys = bounds.keys.sorted()
    return map.keys.sorted().flatMap { file -> [Link] in
      guard let from = bounds[file], let test = map[file] else { return [] }
      return test.uses.compactMap { used in
        guard used != file, let to = bounds[used] else { return nil }
        let others = keys.filter { $0 != file && $0 != used }.compactMap { bounds[$0].map(rect) }
        let points = mapLinkRoute(from: rect(from), to: rect(to), others: others).map { CGPoint(x: $0.x, y: $0.y) }
        return Link(from: from, to: to, points: points)
      }
    }
  }

  private static func rect(_ r: CGRect) -> Rect {
    Rect(x: r.minX, y: r.minY, width: r.width, height: r.height)
  }
}

private struct CardBounds: PreferenceKey {
  static let defaultValue: [String: Anchor<CGRect>] = [:]
  static func reduce(value: inout [String: Anchor<CGRect>], nextValue: () -> [String: Anchor<CGRect>]) {
    value.merge(nextValue()) { $1 }
  }
}

private struct DashedLine: Shape {
  func path(in rect: CGRect) -> Path {
    Path { path in
      path.move(to: CGPoint(x: rect.minX, y: rect.midY))
      path.addLine(to: CGPoint(x: rect.maxX, y: rect.midY))
    }
  }
}

/// Dashed arrows from a card to the cards of the tests it runs, in straight runs with rounded
/// turns that go around the other cards.
private struct UsesArrows: View {
  let links: [ProjectMap.Link]

  var body: some View {
    Canvas { context, _ in
      for link in links {
        let points = link.points
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
        context.stroke(line, with: .color(.secondary), style: StrokeStyle(lineWidth: 1.5, dash: [5, 4]))
        // The head points the way the last run arrives.
        let last = points[points.count - 2]
        let angle = atan2(end.y - last.y, end.x - last.x)
        var head = Path()
        head.move(to: end)
        head.addLine(to: CGPoint(x: end.x - 8 * cos(angle - 0.45), y: end.y - 8 * sin(angle - 0.45)))
        head.addLine(to: CGPoint(x: end.x - 8 * cos(angle + 0.45), y: end.y - 8 * sin(angle + 0.45)))
        head.closeSubpath()
        context.fill(head, with: .color(.secondary))
      }
    }
  }
}

private struct MapCard: View {
  static let width: CGFloat = 208

  let test: ProjectItem
  let info: MapTest?
  let last: SavedRun?
  let quarantine: Quarantined?
  let flaky: FlakyEvidence?
  /// Names of the tests it runs, and of the tests that run it.
  let uses: [String]
  let usedBy: [String]
  let open: () -> Void
  @State private var isHovering = false

  var body: some View {
    Button(action: open) {
      VStack(alignment: .leading, spacing: 4) {
        Text(test.name)
          .font(.headline)
          .lineLimit(1)
        HStack(spacing: 5) {
          Image(systemName: state.symbol)
            .foregroundStyle(state.color)
          Text(state.text)
            .lineLimit(2)
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        if let quarantine {
          Label {
            Text(QuarantineText.describe(quarantine)).lineLimit(2)
          } icon: {
            Image(systemName: RunStatus.quarantined.symbol).foregroundStyle(RunStatus.quarantined.color)
          }
          .font(.caption)
          .foregroundStyle(.secondary)
        }
        if let flaky, last?.status != .flaky {
          Label {
            Text("Flaky").lineLimit(1)
          } icon: {
            Image(systemName: RunStatus.flaky.symbol).foregroundStyle(RunStatus.flaky.color)
          }
          .font(.caption)
          .foregroundStyle(.secondary)
          .help(Text(RunHistory.flakyEvidence(flaky)))
        }
        if let blocks = info?.blocks, blocks > 0 {
          // A short bar per block, up to ten: how big the test is at a glance.
          HStack(spacing: 3) {
            ForEach(0..<min(Int(blocks), 10), id: \.self) { _ in
              RoundedRectangle(cornerRadius: 2)
                .fill(.quaternary)
                .frame(width: 14, height: 5)
            }
          }
          .padding(.top, 4)
        }
      }
      .padding(12)
      .frame(width: Self.width, alignment: .leading)
      .background(.background, in: .rect(cornerRadius: 14))
      .overlay(
        RoundedRectangle(cornerRadius: 14)
          .strokeBorder(isHovering ? AnyShapeStyle(.tint) : AnyShapeStyle(.separator), lineWidth: isHovering ? 2 : 0.5)
      )
      .shadow(color: .black.opacity(0.06), radius: 7, y: 4)
      .contentShape(.rect(cornerRadius: 14))
    }
    .buttonStyle(.plain)
    .onHover { isHovering = $0 }
    .help(Text("Open the test"))
    .accessibilityLabel(Text(test.name))
    .accessibilityValue(Text(verbatim: accessibilityValue))
    .accessibilityHint(Text("Opens the test"))
  }

  private var state: (symbol: String, color: Color, text: String) {
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

  private var accessibilityValue: String {
    var parts = [state.text]
    if let quarantine { parts.append(QuarantineText.describe(quarantine)) }
    if let flaky { parts.append(RunHistory.flakyEvidence(flaky)) }
    if !uses.isEmpty { parts.append(String(localized: "Runs \(uses.formatted(.list(type: .and)))")) }
    if !usedBy.isEmpty { parts.append(String(localized: "Run by \(usedBy.formatted(.list(type: .and)))")) }
    return parts.joined(separator: ". ")
  }
}
