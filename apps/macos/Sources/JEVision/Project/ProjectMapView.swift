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
            ForEach(group.tests) { test in
              let info = map[test.id]
              MapCard(
                test: test, info: info, last: latest[test.id],
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
  }

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

  /// An arrow from each test to each test it runs, between the cards' frames.
  static func links(_ map: [String: MapTest], bounds: [String: CGRect]) -> [Link] {
    map.keys.sorted().flatMap { file -> [Link] in
      guard let from = bounds[file], let test = map[file] else { return [] }
      return test.uses.compactMap { used in
        guard used != file, let to = bounds[used] else { return nil }
        return Link(from: from, to: to)
      }
    }
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

/// Dashed arrows from a card to the cards of the tests it runs: side to side between frames,
/// around the right side within one.
private struct UsesArrows: View {
  let links: [ProjectMap.Link]

  var body: some View {
    Canvas { context, _ in
      for link in links {
        let (a, b) = (link.from, link.to)
        let start: CGPoint
        let end: CGPoint
        let out: CGFloat
        let into: CGFloat
        if b.minX >= a.maxX {
          (start, end) = (CGPoint(x: a.maxX, y: a.midY), CGPoint(x: b.minX, y: b.midY))
          (out, into) = (max(30, (end.x - start.x) / 2), max(30, (end.x - start.x) / 2))
        } else if b.maxX <= a.minX {
          (start, end) = (CGPoint(x: a.minX, y: a.midY), CGPoint(x: b.maxX, y: b.midY))
          (out, into) = (-max(30, (start.x - end.x) / 2), -max(30, (start.x - end.x) / 2))
        } else {
          (start, end) = (CGPoint(x: a.maxX, y: a.midY), CGPoint(x: b.maxX, y: b.midY))
          (out, into) = (36, -36)
        }
        let control2 = CGPoint(x: end.x - into, y: end.y)
        var curve = Path()
        curve.move(to: start)
        curve.addCurve(to: end, control1: CGPoint(x: start.x + out, y: start.y), control2: control2)
        context.stroke(curve, with: .color(.secondary), style: StrokeStyle(lineWidth: 1.5, dash: [5, 4]))
        // The head points the way the curve arrives.
        let angle = atan2(end.y - control2.y, end.x - control2.x)
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
    if !uses.isEmpty { parts.append(String(localized: "Runs \(uses.formatted(.list(type: .and)))")) }
    if !usedBy.isEmpty { parts.append(String(localized: "Run by \(usedBy.formatted(.list(type: .and)))")) }
    return parts.joined(separator: ". ")
  }
}
