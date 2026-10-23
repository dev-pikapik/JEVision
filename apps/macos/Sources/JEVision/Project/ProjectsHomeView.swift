import AppKit
import JEVisionCore
import SwiftUI

/// The main window when no project is open: the projects opened on this Mac, or, before the
/// first one, the ways to start.
struct ProjectsHomeView: View {
  @Environment(ProjectStore.self) private var project

  var body: some View {
    Group {
      if project.recent.isEmpty { FirstProjectView() } else { RecentProjectsView() }
    }
    .onAppear(perform: project.refreshSummaries)
    // Folders change while the app is in the background: look again when it comes back.
    .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
      project.refreshSummaries()
    }
  }

  nonisolated static let cardWidth: CGFloat = 250
  nonisolated static let spacing: CGFloat = 16

  /// How many cards fit side by side in `width`.
  nonisolated static func columns(for width: CGFloat) -> Int {
    max(1, Int((width + spacing) / (cardWidth + spacing)))
  }

  /// Where an arrow key leads from the card of `path` when `sections` are laid out `columns`
  /// wide: left and right go through the cards in order, up and down to the card above or below,
  /// across sections too. The first card when none is picked.
  nonisolated static func step(
    from path: String?, _ direction: MoveCommandDirection, in sections: [[String]], columns: Int
  ) -> String? {
    let rows = sections.flatMap { section in
      stride(from: 0, to: section.count, by: columns).map { Array(section[$0..<min($0 + columns, section.count)]) }
    }
    let order = rows.flatMap { $0 }
    guard let path, let index = order.firstIndex(of: path), let row = rows.firstIndex(where: { $0.contains(path) }),
      let column = rows[row].firstIndex(of: path)
    else { return order.first }
    switch direction {
    case .left: return order[max(index - 1, 0)]
    case .right: return order[min(index + 1, order.count - 1)]
    case .up: return row > 0 ? rows[row - 1][min(column, rows[row - 1].count - 1)] : path
    case .down: return row + 1 < rows.count ? rows[row + 1][min(column, rows[row + 1].count - 1)] : path
    @unknown default: return path
    }
  }
}

/// Pinned and recent projects as cards, with search.
private struct RecentProjectsView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.layoutDirection) private var layoutDirection
  @State private var query = ""
  @State private var picked: String?
  @State private var columns = 1
  @FocusState private var searching: Bool
  @FocusState private var gridFocused: Bool

  private var sections: [[RecentProject]] {
    let found = recentProjectsMatching(list: project.recent, query: query)
    return [found.filter(\.pinned), found.filter { !$0.pinned }]
  }

  var body: some View {
    let sections = sections
    Group {
      if sections.allSatisfy(\.isEmpty) {
        ContentUnavailableView.search(text: query)
      } else {
        ScrollViewReader { proxy in
          ScrollView {
            VStack(alignment: .leading, spacing: 28) {
              section(Text("Pinned"), sections[0])
              section(Text("Recent"), sections[1])
            }
            .padding(24)
          }
          .onGeometryChange(for: Int.self) { ProjectsHomeView.columns(for: $0.size.width - 48) } action: { columns = $0 }
          .onChange(of: picked) { _, path in
            if let path { withAnimation { proxy.scrollTo(path) } }
          }
        }
      }
    }
    .focusable()
    .focusEffectDisabled()
    .focused($gridFocused)
    .onMoveCommand { direction in
      var direction = direction
      if layoutDirection == .rightToLeft, direction == .left || direction == .right {
        direction = direction == .left ? .right : .left
      }
      picked = ProjectsHomeView.step(from: picked, direction, in: sections.map { $0.map(\.path) }, columns: columns)
    }
    .onKeyPress(.return) {
      guard let picked else { return .ignored }
      project.openRecent(picked)
      return .handled
    }
    .onKeyPress(.delete) {
      guard let picked else { return .ignored }
      remove(picked)
      return .handled
    }
    .onAppear { gridFocused = true }
    .onChange(of: query) {
      let found = self.sections.flatMap { $0 }
      if !found.contains(where: { $0.path == picked }) { picked = query.isEmpty ? nil : found.first?.path }
    }
    .searchable(text: $query, placement: .toolbar, prompt: Text("Search projects"))
    .searchFocused($searching)
    .onSubmit(of: .search) {
      if let first = self.sections.flatMap({ $0 }).first { project.openRecent(first.path) }
    }
    .background {
      // ⌘F goes to the search field, as in the Finder.
      Button("Search projects") { searching = true }
        .keyboardShortcut("f")
        .opacity(0)
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
    .toolbar {
      ToolbarItem(placement: .primaryAction) {
        Button(action: project.choose) { Label("Open…", systemImage: "folder") }
      }
      ToolbarItem(placement: .primaryAction) {
        Button { project.newProject = .new } label: { Label("New Project", systemImage: "plus") }
          .buttonStyle(.glassProminent)
      }
    }
  }

  @ViewBuilder
  private func section(_ title: Text, _ list: [RecentProject]) -> some View {
    if !list.isEmpty {
      VStack(alignment: .leading, spacing: 12) {
        title
          .font(.title3.weight(.semibold))
          .accessibilityAddTraits(.isHeader)
        LazyVGrid(
          columns: Array(repeating: GridItem(.flexible(), spacing: ProjectsHomeView.spacing, alignment: .top), count: columns),
          spacing: ProjectsHomeView.spacing
        ) {
          ForEach(list, id: \.path) { entry in
            ProjectCard(
              entry: entry, summary: project.summaries[entry.path], isPicked: picked == entry.path,
              pick: {
                picked = entry.path
                gridFocused = true
              },
              remove: { remove(entry.path) }
            )
            .id(entry.path)
          }
        }
      }
    }
  }

  /// Takes the project off the list and picks the card next to it.
  private func remove(_ path: String) {
    let order = sections.flatMap { $0 }.map(\.path)
    if picked == path, let index = order.firstIndex(of: path) {
      picked = order.indices.contains(index + 1) ? order[index + 1] : index > 0 ? order[index - 1] : nil
    }
    project.removeRecent(path)
  }
}

/// One recent project: its map in small, its name and folder, and what's in it.
private struct ProjectCard: View {
  @Environment(ProjectStore.self) private var project
  let entry: RecentProject
  let summary: ProjectSummary?
  let isPicked: Bool
  let pick: () -> Void
  let remove: () -> Void
  @State private var hovering = false

  private var isMissing: Bool { summary?.exists == false }

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      preview
        .frame(maxWidth: .infinity)
        .frame(height: 96)
        .background(.quinary, in: .rect(cornerRadius: 9))
        .opacity(isMissing ? 0.6 : 1)
      HStack(alignment: .top, spacing: 6) {
        VStack(alignment: .leading, spacing: 2) {
          Text(verbatim: entry.name)
            .font(.headline)
            .lineLimit(1)
          Text(verbatim: ProjectStore.shortPath(entry.path))
            .font(.caption)
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .truncationMode(.middle)
        }
        .opacity(isMissing ? 0.6 : 1)
        Spacer(minLength: 0)
        pin
      }
      if isMissing {
        HStack(spacing: 8) {
          Button("Locate…") { project.locate(entry.path) }
          Button("Remove from List", action: remove)
        }
        .controlSize(.small)
      } else {
        Text(verbatim: facts)
          .font(.caption)
          .foregroundStyle(.secondary)
          .lineLimit(1)
      }
    }
    .padding(12)
    .background(.background, in: .rect(cornerRadius: 14))
    .overlay(
      RoundedRectangle(cornerRadius: 14)
        .strokeBorder(isPicked ? AnyShapeStyle(.tint) : AnyShapeStyle(.separator), lineWidth: isPicked ? 2 : 0.5)
    )
    .shadow(color: .black.opacity(0.06), radius: 7, y: 4)
    .contentShape(.rect(cornerRadius: 14))
    .onHover { hovering = $0 }
    .onTapGesture(count: 2) { project.openRecent(entry.path) }
    .simultaneousGesture(TapGesture().onEnded(pick))
    .contextMenu { menu }
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(Text(verbatim: entry.name))
    .accessibilityValue(Text(verbatim: spokenValue))
    .accessibilityAddTraits(isPicked ? [.isButton, .isSelected] : .isButton)
    .accessibilityAction { project.openRecent(entry.path) }
    .accessibilityAction(named: entry.pinned ? Text("Unpin") : Text("Pin")) {
      project.setPinned(entry.path, !entry.pinned)
    }
    .accessibilityAction(named: isMissing ? Text("Locate…") : Text("Show in Finder")) {
      if isMissing { project.locate(entry.path) } else { showInFinder() }
    }
    .accessibilityAction(named: Text("Remove from List"), remove)
  }

  @ViewBuilder
  private var preview: some View {
    if isMissing {
      VStack(spacing: 6) {
        Image(systemName: "questionmark.folder")
          .font(.title)
          .foregroundStyle(.secondary)
          .accessibilityHidden(true)
        Text("Folder can’t be found")
          .font(.caption.weight(.medium))
          .foregroundStyle(.secondary)
      }
    } else if let summary {
      ProjectThumbnail(summary: summary)
    }
  }

  /// The pin shows on the pinned cards, and on the others when the pointer is over them.
  private var pin: some View {
    Button {
      project.setPinned(entry.path, !entry.pinned)
    } label: {
      Image(systemName: entry.pinned ? "pin.fill" : "pin")
    }
    .buttonStyle(.borderless)
    .foregroundStyle(entry.pinned ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
    .help(entry.pinned ? Text("Unpin") : Text("Pin"))
    .opacity(entry.pinned || hovering || isPicked ? 1 : 0)
    .accessibilityHidden(true)
  }

  @ViewBuilder
  private var menu: some View {
    Button("Open") { project.openRecent(entry.path) }
    if isMissing {
      Button("Locate…") { project.locate(entry.path) }
    } else {
      Button("Show in Finder", action: showInFinder)
    }
    Button { project.setPinned(entry.path, !entry.pinned) } label: { entry.pinned ? Text("Unpin") : Text("Pin") }
    Divider()
    Button("Remove from List", action: remove)
  }

  /// "12 tests · main · Failed 2 hr. ago"
  private var facts: String {
    guard let summary else { return " " }
    var parts = [String(localized: "\(Int(summary.testCount)) tests")]
    if let branch = summary.branch { parts.append(branch) }
    if let run = summary.lastRun {
      let status = RunStatus(run.verdict).title
      if let finished = run.finishedAt {
        let when = Date(timeIntervalSince1970: TimeInterval(finished))
          .formatted(.relative(presentation: .named, unitsStyle: .abbreviated).locale(.app))
        parts.append("\(status) \(when)")
      } else {
        parts.append(status)
      }
    }
    return parts.joined(separator: " · ")
  }

  private var spokenValue: String {
    var parts = [ProjectStore.shortPath(entry.path)]
    parts.append(isMissing ? String(localized: "Folder can’t be found") : facts)
    return parts.joined(separator: ". ")
  }

  private func showInFinder() {
    NSWorkspace.shared.activateFileViewerSelecting([URL(filePath: entry.path)])
  }
}

/// The project's map in small: a frame for each folder and a dot for each test, in the colour and
/// shape of its last run, grey when it hasn't run. Tests at the top of the project have no frame.
private struct ProjectThumbnail: View {
  let summary: ProjectSummary

  var body: some View {
    WrapLayout(spacing: 6) {
      // More than fit in the card: the rest would be cut off anyway.
      ForEach(summary.folders.prefix(24), id: \.path) { folder in
        if folder.path.isEmpty {
          dots(folder.tests)
        } else {
          dots(folder.tests)
            .padding(5)
            .overlay(RoundedRectangle(cornerRadius: 5).strokeBorder(.separator))
        }
      }
    }
    .padding(10)
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    .clipped()
    .accessibilityHidden(true)
  }

  /// Up to six dots a row; what doesn't fit in the card is cut off.
  private func dots(_ tests: [ProjectTest]) -> some View {
    let rows = stride(from: 0, to: min(tests.count, 24), by: 6).map { Array(tests[$0..<min($0 + 6, tests.count)]) }
    return VStack(alignment: .leading, spacing: 3) {
      ForEach(rows.indices, id: \.self) { row in
        HStack(spacing: 3) {
          ForEach(rows[row], id: \.file) { test in
            let status = test.last.map(RunStatus.init)
            Image(systemName: status?.dot ?? "circle.fill")
              .font(.system(size: 6))
              .foregroundStyle(status?.color ?? Color(nsColor: .tertiaryLabelColor))
              .frame(width: 7, height: 7)
          }
        }
      }
    }
  }
}

/// Before the first project: start a new one or open a folder, and quieter ways to begin.
private struct FirstProjectView: View {
  @Environment(ProjectStore.self) private var project

  var body: some View {
    VStack(spacing: 28) {
      HStack(spacing: 20) {
        ChoiceCard(
          symbol: "plus", title: Text("New Project"), message: Text("Start a project in a new folder.")
        ) { project.newProject = .new }
        ChoiceCard(
          symbol: "folder", title: Text("Open Project"), message: Text("Choose a folder that already has tests.")
        ) { project.choose() }
      }
      HStack(spacing: 24) {
        Button("Clone Repository…") { project.newProject = .clone }
        Button("Open Example Project") { project.newProject = .example }
      }
      .buttonStyle(.link)
    }
    .padding(32)
    .frame(maxWidth: .infinity, maxHeight: .infinity)
  }
}

/// A large card that is a button: an icon, a title and a sentence.
private struct ChoiceCard: View {
  let symbol: String
  let title: Text
  let message: Text
  let perform: () -> Void
  @State private var hovering = false

  var body: some View {
    Button(action: perform) {
      VStack(spacing: 12) {
        IconTile(symbol: symbol, color: .accentColor, size: 56)
        title
          .font(.title3.weight(.semibold))
        message
          .font(.callout)
          .foregroundStyle(.secondary)
          .multilineTextAlignment(.center)
          .fixedSize(horizontal: false, vertical: true)
      }
      .padding(20)
      .frame(width: 240, height: 190)
      .background(.background, in: .rect(cornerRadius: 16))
      .overlay(
        RoundedRectangle(cornerRadius: 16)
          .strokeBorder(hovering ? AnyShapeStyle(.tint) : AnyShapeStyle(.separator), lineWidth: hovering ? 2 : 0.5)
      )
      .shadow(color: .black.opacity(0.06), radius: 7, y: 4)
      .contentShape(.rect(cornerRadius: 16))
    }
    .buttonStyle(.plain)
    .onHover { hovering = $0 }
  }
}
