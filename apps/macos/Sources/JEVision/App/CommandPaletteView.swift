import JEVisionCore
import SwiftUI

/// ⌘K: one search field for every command, every block type and every test. ↑↓ move, Return
/// runs or adds, Escape closes.
struct CommandPaletteView: View {
  static let size = CGSize(width: 560, height: 460)
  @Environment(ProjectStore.self) private var project
  @Environment(RunCenter.self) private var runs
  @Environment(RecordCenter.self) private var records
  /// The window's own actions: the palette's panel is outside the scene that has them.
  let openWindow: OpenWindowAction
  let openSettings: OpenSettingsAction
  @State private var query = ""
  @State private var highlighted: PaletteAction?
  @FocusState private var searchFocused: Bool

  /// Every row is one line of title and one of detail.
  private static let rowHeight = 45.0

  /// The whole list; while searching, only as tall as the results, like Spotlight.
  private var listHeight: Double {
    let full = Self.size.height - 56
    guard isSearching else { return full }
    return min(full, Double(max(entries.count, 3)) * Self.rowHeight + 16)
  }

  private var isSearching: Bool { !query.trimmingCharacters(in: .whitespaces).isEmpty }

  private var all: [PaletteEntry] { CommandPalette.entries(state) }

  /// The entries in the order the rows are drawn.
  private var entries: [PaletteEntry] { isSearching ? CommandPalette.search(query, in: all) : all }

  var body: some View {
    VStack(spacing: 0) {
      HStack {
        Image(systemName: "magnifyingglass").foregroundStyle(.secondary).accessibilityHidden(true)
        TextField(text: $query, prompt: Text("Search commands, blocks and tests")) { Text("Search") }
          .textFieldStyle(.plain)
          .font(.title3)
          .focused($searchFocused)
          .onSubmit(submit)
      }
      .padding(14)
      Divider()
      ScrollViewReader { proxy in
        List(selection: $highlighted) {
          if isSearching {
            ForEach(entries, content: row)
          } else {
            section(Text("Commands"), kind: .command)
            section(Text("Add a Block"), kind: .block)
            section(Text("Tests"), kind: .test)
            section(Text("Boards"), kind: .board)
            section(Text("Projects"), kind: .project)
          }
        }
        .listStyle(.sidebar)
        .scrollContentBackground(.hidden)
        .frame(maxHeight: listHeight)
        .overlay {
          if entries.isEmpty { ContentUnavailableView.search(text: query) }
        }
        .onChange(of: highlighted) { if let highlighted { proxy.scrollTo(highlighted) } }
      }
    }
    .frame(width: Self.size.width)
    .adaptiveGlass(in: .rect(cornerRadius: 22))
    .onAppear {
      searchFocused = true
      highlighted = entries.first?.id
    }
    .onChange(of: query) { highlighted = entries.first?.id }
    .onKeyPress(.downArrow) { move(1) }
    .onKeyPress(.upArrow) { move(-1) }
    .onKeyPress(.escape) {
      close()
      return .handled
    }
    .accessibilityElement(children: .contain)
    .accessibilityLabel(Text("Command palette"))
    .accessibilityAddTraits(.isModal)
  }

  @ViewBuilder private func section(_ title: Text, kind: PaletteEntry.Kind) -> some View {
    let shown = entries.filter { $0.kind == kind }
    if !shown.isEmpty {
      Section { ForEach(shown, content: row) } header: { title }
    }
  }

  private func row(_ entry: PaletteEntry) -> some View {
    PaletteRow(
      symbol: entry.symbol, color: entry.color, title: Text(verbatim: entry.title), detail: Text(verbatim: entry.detail)
    ) {
      if let shortcut = entry.shortcut {
        Text(verbatim: Shortcuts.symbols(shortcut))
          .font(.callout.monospaced())
          .foregroundStyle(.secondary)
          .accessibilityLabel(Text("Shortcut \(Shortcuts.symbols(shortcut))"))
      } else if isSearching {
        kindTitle(entry.kind)
          .font(.caption)
          .foregroundStyle(.secondary)
      }
    }
    .tag(entry.id)
    .id(entry.id)
    .contentShape(.rect)
    .onTapGesture { perform(entry.action) }
    .accessibilityAddTraits(.isButton)
    .accessibilityAction { perform(entry.action) }
  }

  private func kindTitle(_ kind: PaletteEntry.Kind) -> Text {
    switch kind {
    case .command: Text("Command")
    case .block: Text("Block")
    case .test: Text("Test")
    case .board: Text("Board")
    case .project: Text("Project")
    }
  }

  private func move(_ step: Int) -> KeyPress.Result {
    let ids = rowOrder
    guard !ids.isEmpty else { return .handled }
    let index = highlighted.flatMap(ids.firstIndex) ?? (step > 0 ? -1 : ids.count)
    highlighted = ids[min(max(index + step, 0), ids.count - 1)]
    return .handled
  }

  /// The ids in the order the rows are drawn: by section when not searching.
  private var rowOrder: [PaletteAction] {
    let shown = entries
    if isSearching { return shown.map(\.id) }
    return [PaletteEntry.Kind.command, .block, .test, .board, .project].flatMap { kind in shown.filter { $0.kind == kind }.map(\.id) }
  }

  private func submit() {
    if let action = highlighted ?? rowOrder.first { perform(action) }
  }

  private func close() {
    project.showsCommandPalette = false
  }

  // MARK: What can be done

  private var state: PaletteState {
    let document = project.document
    let flow = document?.flow
    let selection = flow == nil ? nil : document?.selection
    var state = PaletteState()
    state.hasProject = project.status == .ready
    state.hasTest = flow != nil && document?.isReadOnly != true
    state.canRun = !runs.isBusy && !records.isRecording && project.rootURL != nil && flow != nil
      && document?.beforeDraft == nil
    state.selection = selection
    if let selection, let document {
      state.selectionIsIf = document.block(selection)?.kind.typeName == "if"
      state.selectionHasLinks = document.hasLinks(selection)
    }
    state.hasPickedBlocks = flow != nil && document?.picked.contains { $0.blockID != nil } == true
    let tests = project.items.flatMap(\.tests)
    state.hasTests = !project.chosen(from: project.items).isEmpty
    state.filtersTests = project.filtersTests
    let last = lastRun
    state.hasLastRun = last != nil
    state.lastRunFailed = last?.report.verdict == .failed
    state.hasRuns = !project.runs.isEmpty
    state.tests = tests.map { ($0.id, $0.name, $0.parentPath, $0.keywords) }
    state.boards = project.boards.names
    if project.showsBoard, let name = project.boards.current, let board = project.boards.board {
      state.boardInView = name
      let on = Set(board.nodes.compactMap { node -> String? in if case .file(let file) = node.kind { file } else { nil } })
      state.boardLacksTests = tests.contains { !on.contains($0.id) }
      state.boardItems = board.nodes.compactMap { node in
        switch node.kind {
        case .group(let label, _, _): (node.id, label, String(localized: "Frame on \(name)"))
        case .text(let text):
          (node.id, String(text.prefix { $0 != "\n" }.prefix(80)), String(localized: "Note on \(name)"))
        case .link(let url): (node.id, url, String(localized: "Link on \(name)"))
        case .file(let file):
          isBoardImage(file: file) ? (node.id, (file as NSString).lastPathComponent, String(localized: "Picture on \(name)")) : nil
        }
      }.filter { !$0.1.trimmingCharacters(in: .whitespaces).isEmpty }
    }
    state.library = project.library.entries
    state.recentProjects = project.openableRecent.filter { $0.path != project.rootURL?.path }.map {
      ($0.path, $0.name, ProjectStore.shortPath($0.path))
    }
    if case .tracked(let status) = project.git.state {
      state.git = PaletteState.Git(
        isTracked: true, hasChanges: !status.changes.isEmpty, hasCommits: status.hasCommits,
        hasUpstream: status.upstream != nil, hasRemote: status.remote != nil,
        onGitHub: status.github != nil && status.branch != nil, isBusy: project.git.activity != nil)
    }
    return state
  }

  /// The newest run of the open test.
  private var lastRun: SavedRun? {
    guard let root = project.rootURL, let document = project.document, let flow = document.flow else { return nil }
    return LastRun(file: RelativePath.from(root, to: document.url), flow: flow, runs: project.runs, pinned: nil).own
  }

  private func perform(_ action: PaletteAction) {
    close()
    let document = project.document
    let root = project.rootURL
    switch action {
    case .newTest: project.isCreatingTest = true
    case .describeTest:
      if let document, document.flow != nil, !document.isReadOnly {
        if document.drafter == nil { document.drafter = TestDrafter() }
      } else {
        project.isCreatingTest = true
      }
    case .newProject: project.newProject = .new
    case .cloneRepository: project.newProject = .clone
    case .openProject: project.choose()
    case .projects: project.close()
    case .openRecentProject(let path): project.openRecent(path)
    case .settings: openSettings()
    case .run, .runStepByStep, .runFromSelection:
      guard let document, let root else { return }
      runs.run(
        document, in: root, from: action == .runFromSelection ? document.selection : nil,
        stepByStep: action == .runStepByStep)
    case .runAll:
      guard let root else { return }
      runs.runAll(project.chosen(from: project.items).map(\.url), in: root, title: project.runAllTitle)
    case .record: if let document { records.record(document) }
    case .schedules: project.schedules.isShowingSheet = true
    case .installBlock: project.installBlock()
    case .customBlocks: project.showsCustomBlocks = true
    case .addBlock(let type): document?.insert(type, at: document?.slotAfterSelection)
    case .addFromLibrary(let entry): document?.add(entry, at: document?.slotAfterSelection)
    case .saveToLibrary(let kind): document?.savingToLibrary = kind
    case .addCondition: document?.addCondition()
    case .arrange: document?.arrange()
    case .disconnect: if let document, let id = document.selection { document.disconnect(id) }
    case .openReport: if let run = lastRun { project.showRun(run.url) }
    case .newBoard:
      do {
        let name = try project.boards.create(String(localized: "New Board"))
        project.showBoard(name)
        project.boards.renaming = name
      } catch {
        NSSound.beep()
      }
    case .addToBoard(let addition): project.boards.request = .add(addition)
    case .showBoard(let name): project.showBoard(name)
    case .showOnBoard(let id): project.boards.request = .reveal(id)
    case .reportBug: if let run = lastRun { project.bugReportRun = run.dir }
    case .testStatistics:
      guard let root, let document else { return }
      project.statisticsTest = RelativePath.from(root, to: document.url)
      openWindow(id: SceneID.statistics)
    case .projectStatistics:
      project.statisticsTest = nil
      openWindow(id: SceneID.statistics)
    case .commit: openWindow(id: SceneID.commit)
    case .newPullRequest: project.git.isComposingPullRequest = true
    case .pullRequests: openWindow(id: SceneID.pullRequests)
    case .pull: project.git.pull()
    case .push: project.git.push()
    case .openTest(let id):
      project.sidebar = .tests
      project.selection = id
    }
  }
}
