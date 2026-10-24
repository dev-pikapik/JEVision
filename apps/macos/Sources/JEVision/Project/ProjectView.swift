import JEVisionCore
import SwiftUI

struct ProjectView: View {
  /// Starts the New Test sheet to record into the test it creates; for the empty project.
  let recordFirstTest: () -> Void
  @Environment(ProjectStore.self) private var project
  @Environment(RunCenter.self) private var runs
  @Environment(RecordCenter.self) private var records
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @State private var search = SidebarSearch()
  @State private var catalog = ProjectCatalog()
  @State private var expandedProjects: Set<String> = []
  @State private var renaming: ProjectItem?
  @State private var newName = ""
  @State private var newBoardName = ""
  @State private var quarantining: QuarantineRequest?
  @State private var failure: String?
  /// Open folders; the folders around the selected test open by themselves.
  @State private var expanded: Set<ProjectItem.ID> = []

  var body: some View {
    @Bindable var project = project
    @Bindable var search = search
    NavigationSplitView {
      testList
      .scrollContentBackground(.hidden)
      // Bars, not insets: the list scrolls under them, and their edge effect keeps them readable.
      .safeAreaBar(edge: .top, spacing: 0) {
        VStack(spacing: 0) {
          HStack(spacing: 8) {
            Button("Projects", systemImage: "square.grid.2x2") { project.close() }
              .labelStyle(.iconOnly).buttonStyle(.borderless)
            Button("Open…", systemImage: "folder.badge.plus", action: project.choose)
              .labelStyle(.iconOnly).buttonStyle(.borderless)
            Button("New Project", systemImage: "plus") { project.newProject = .new }
              .labelStyle(.iconOnly).buttonStyle(.borderless)
            Spacer()
          }.padding(.horizontal, 10).padding(.bottom, 8)
          HStack(spacing: 8) {
            Picker("Show", selection: $project.sidebar) {
              Text("Tests").tag(ProjectStore.Sidebar.tests)
              Text("Runs").tag(ProjectStore.Sidebar.runs)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            if project.sidebar == .tests, !project.items.isEmpty { TestFilterMenu() }
          }
          .padding(.horizontal, 10)
          .padding(.bottom, 6)
          if project.sidebar == .tests, project.filtersTests { TestFilterBar() }
        }
      }
      .safeAreaBar(edge: .bottom, spacing: 0) {
        VStack(spacing: 0) {
          if project.rootURL != nil {
            EnvironmentChip()
            ScheduleChip()
            if let status = project.git.status { GitChip(status: status) }
          }
        }
      }
      .searchable(
        text: $search.text, placement: .sidebar,
        prompt: project.sidebar == .runs ? Text("Filter runs by test") : Text("Search projects and tests"))
      .navigationSplitViewColumnWidth(min: 240, ideal: 340, max: 600)
    } detail: {
      detail
        .overlay(alignment: .bottom) {
          if let result = runs.batchResult {
            BatchResultBar(result: result)
              .transition(reduceMotion ? .opacity : .move(edge: .bottom).combined(with: .opacity))
          }
        }
        .animation(reduceMotion ? nil : .snappy, value: runs.batchResult)
        .overlay(alignment: .top) {
          if !project.unrestored.isEmpty, !project.restoreDeclined {
            BlockRestoreBar(count: project.unrestored.count, review: project.restoreBlocks) {
              project.restoreDeclined = true
            }
            .transition(reduceMotion ? .opacity : .move(edge: .top).combined(with: .opacity))
          }
        }
        .animation(reduceMotion ? nil : .snappy, value: project.unrestored.isEmpty || project.restoreDeclined)
    }
    .onChange(of: runs.result) { _, result in
      // Run Again from a report: the new run's report takes its place.
      if project.sidebar == .runs, project.selectedRun != nil, let run = result?.run { project.showRun(run) }
    }
    .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
      project.git.refresh()
      project.refreshSummaries()
      for entry in project.recent where expandedProjects.contains(entry.path) { catalog.load(entry, refresh: true) }
    }
    .onChange(of: project.rootURL, initial: true) {
      if let path = project.rootURL?.path { expandedProjects.insert(path) }
    }
    .onChange(of: search.isSearching) {
      if search.isSearching { for entry in project.recent { catalog.load(entry, refresh: true) } }
    }
    .quarantineSheet($quarantining)
    .alert("Rename Test", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
      TextField("Name", text: $newName)
      Button("Rename") {
        guard let item = renaming else { return }
        attempt { try project.rename(item, to: newName) }
      }
      Button("Cancel", role: .cancel) {}
    }
    .onChange(of: project.boards.renaming) { if let name = project.boards.renaming { newBoardName = name } }
    .alert(
      "Rename Board",
      isPresented: Binding(get: { project.boards.renaming != nil }, set: { if !$0 { project.boards.renaming = nil } })
    ) {
      TextField("Name", text: $newBoardName)
      Button("Rename") {
        guard let name = project.boards.renaming else { return }
        attempt { try project.boards.rename(name, to: newBoardName) }
      }
      Button("Cancel", role: .cancel) {}
    }
    .alert(
      "Something went wrong", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } }),
      presenting: failure
    ) { _ in
      Button("OK") {}
    } message: { Text($0) }
  }

  @ViewBuilder private var detail: some View {
    if project.status == .closed {
      ProjectsHomeView()
    } else if project.status == .unavailable {
      EmptyStateView(
        symbol: "lock.trianglebadge.exclamationmark", title: Text("Can’t open “\(project.name)”"),
        message: Text("JEVision can’t read this folder. Allow it in System Settings › Privacy & Security › Files and Folders, then try again."),
        action: Text("Try Again"), perform: { if let url = project.rootURL { project.openProject(url) } },
        secondAction: Text("Close Project"), performSecond: project.close)
    } else if project.status == .loading {
      ProgressView().accessibilityLabel(Text("Looking for tests"))
    } else if project.sidebar == .runs {
      if let dir = project.selectedRun {
        RunReportView(dir: dir)
          .id(dir)
      } else if project.runs.isEmpty {
        ContentUnavailableView(
          "Nothing to show yet", systemImage: "clock.arrow.circlepath",
          description: Text("Run a test, and its report shows every step, with screenshots."))
      } else {
        ContentUnavailableView(
          "Select a run", systemImage: "clock.arrow.circlepath",
          description: Text("Its report shows every step, with screenshots."))
      }
    } else if project.items.isEmpty {
      EmptyStateView(
        symbol: "record.circle",
        title: Text("No tests yet"),
        message: Text("Record what you do in an app — JEVision turns every step into a block. Or start with an empty test and add the steps yourself."),
        action: Text("Record First Test…"), perform: recordFirstTest,
        secondAction: Text("New Test…"), performSecond: { project.isCreatingTest = true },
        actionID: AccessibilityID.Project.recordFirstTest, secondActionID: AccessibilityID.Project.newTest)
    } else if project.showsBoard {
      BoardView()
    } else if let document = project.document {
      EditorView(document: document)
        .id(document.url)
    } else if let id = project.selection, let item = project.items.item(withID: id) {
      ItemDetailView(item: item)
    } else {
      ContentUnavailableView("Select a test", systemImage: "flowchart")
    }
  }

  private var testList: some View {
    @Bindable var project = project
    let shown = project.filtered(project.items)
    let selection = Binding(get: { project.sidebarSelection }, set: project.selectSidebar)
    return List(selection: selection) {
      Section("Projects") {
        ForEach(listedProjects, id: \.path) { entry in
          projectFolder(entry, shown: shown)
        }
        if search.isSearching, listedProjects.isEmpty { ContentUnavailableView.search(text: search.query) }
      }
    }
      // A fresh list for search results: reusing the tree's rows left a ghost of the selected one.
      .id(search.isSearching)
      .onChange(of: shown, initial: true) { search.search(in: shown) }
      .onChange(of: project.selection, initial: true) {
        guard let id = project.selection else { return }
        var folder = ""
        for part in id.split(separator: "/").dropLast() {
          folder = folder.isEmpty ? String(part) : folder + "/" + part
          expanded.insert(folder)
        }
      }
      .onDeleteCommand {
        if project.sidebar == .tests, let id = project.selection, let item = project.items.item(withID: id), item.kind == .test { trash(item) }
      }
  }

  private var listedProjects: [RecentProject] {
    guard search.isSearching else { return project.recent }
    return project.recent.filter { entry in
      if !recentProjectsMatching(list: [entry], query: search.query).isEmpty { return true }
      if project.rootURL?.path == entry.path { return !search.found.isEmpty }
      guard let snapshot = catalog.snapshots[entry.path] else { return true }
      return !ProjectScanner.tests(in: snapshot.items, matching: search.query).isEmpty
    }
  }

  private func projectFolder(_ entry: RecentProject, shown: [ProjectItem]) -> some View {
    let active = project.rootURL?.path == entry.path
    let matchesProject = search.isSearching && !recentProjectsMatching(list: [entry], query: search.query).isEmpty
    let binding = Binding(
      get: { expandedProjects.contains(entry.path) || search.isSearching },
      set: { open in
        if open { expandedProjects.insert(entry.path); catalog.load(entry, refresh: true) }
        else { expandedProjects.remove(entry.path) }
      })
    return DisclosureGroup(isExpanded: binding) {
      if active {
        if project.sidebar == .runs {
          RunsList(query: search.text, embedded: true)
        } else if search.isSearching && !matchesProject {
          ForEach(search.found) { row($0, showsPath: true) }
        } else {
          ForEach(project.boards.names, id: \.self) { boardRow($0) }
          ForEach(shown) { tree($0) }
          if shown.isEmpty, project.filtersTests {
            Text("No test has a card that fits the filter: \(TestFilterText.summary(project.filter)).")
              .font(.caption).foregroundStyle(.secondary)
            Button("Clear Filter") { project.filter = TestFilter() }.buttonStyle(.borderless)
          } else if shown.isEmpty {
            Button("New Test…") { project.isCreatingTest = true }.buttonStyle(.borderless)
              .disabled(project.status != .ready)
          }
          Button("New Board", systemImage: "plus", action: newBoard).buttonStyle(.borderless)
            .help(Text("Make a new board"))
            .disabled(project.status != .ready)
        }
      } else if let snapshot = catalog.snapshots[entry.path] {
        if let problem = snapshot.problem {
          Text(problem).font(.caption).foregroundStyle(.secondary)
          Button("Locate…") { project.openRecent(entry.path) }.buttonStyle(.borderless)
        } else if search.isSearching && !matchesProject {
          ForEach(ProjectScanner.tests(in: snapshot.items, matching: search.query)) { item in
            catalogTest(item, in: entry)
          }
        } else if snapshot.items.isEmpty {
          Text("No tests yet").foregroundStyle(.secondary)
        } else {
          ForEach(snapshot.items) { catalogTree($0, in: entry) }
        }
      } else { ProgressView().controlSize(.small) }
    } label: {
      Label {
        HStack {
          Text(project.menuTitle(entry)).fontWeight(active ? .semibold : .regular)
          if entry.pinned { Image(systemName: "pin.fill").imageScale(.small).foregroundStyle(.secondary) }
        }
      } icon: { Image(systemName: active ? "folder.fill" : "folder") }
      .help(ProjectStore.shortPath(entry.path))
      .accessibilityIdentifier("project.folder." + entry.path)
      .contextMenu {
        Button("Open") { project.openRecent(entry.path) }
        Button(entry.pinned ? "Unpin" : "Pin") { project.setPinned(entry.path, !entry.pinned) }
        Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([URL(filePath: entry.path)]) }
        Divider()
        if active { Button("Close Project", action: project.close) }
        Button("Remove from List") { project.removeRecent(entry.path) }.disabled(active)
      }
    }
    .tag(ProjectSidebarSelection.project(entry.path))
    .task(id: entry.path) { if !active, expandedProjects.contains(entry.path) { catalog.load(entry) } }
  }

  private func catalogTree(_ item: ProjectItem, in entry: RecentProject) -> AnyView {
    if item.kind == .folder {
      return AnyView(DisclosureGroup {
        ForEach(item.children) { catalogTree($0, in: entry) }
      } label: { Label(item.name, systemImage: "folder") }
        .tag(ProjectSidebarSelection.item(project: entry.path, file: item.id)))
    }
    return AnyView(catalogTest(item, in: entry))
  }

  private func catalogTest(_ item: ProjectItem, in entry: RecentProject) -> some View {
    Label(item.name, systemImage: "flowchart")
      .tag(ProjectSidebarSelection.item(project: entry.path, file: item.id))
      .help(item.parentPath)
  }

  private func tree(_ item: ProjectItem) -> AnyView {
    guard let children = item.outlineChildren else { return AnyView(row(item, showsPath: false)) }
    let isExpanded = Binding(
      get: { expanded.contains(item.id) },
      set: { if $0 { expanded.insert(item.id) } else { expanded.remove(item.id) } })
    return AnyView(
      DisclosureGroup(isExpanded: isExpanded) {
        ForEach(children) { tree($0) }
      } label: {
        row(item, showsPath: false)
      })
  }

  private func row(_ item: ProjectItem, showsPath: Bool) -> some View {
    SidebarRow(
      item: item, showsPath: showsPath, status: project.latestRuns[item.id]?.status, quarantine: project.quarantine(of: item.id),
      flaky: project.flaky[item.id], suggestion: project.quarantineSuggestion(for: item.id), run: canRun ? { run(item) } : nil
    )
      .tag(ProjectSidebarSelection.item(project: project.rootURL?.path ?? "", file: item.id))
      .contextMenu {
        Button(item.kind == .test ? "Run" : "Run Folder") { run(item) }
          .disabled(!canRun)
        if item.kind == .folder {
          RunModePicker(folder: item.id) { failure = $0 }
        }
        Divider()
        if item.kind == .test {
          Button("Rename…") {
            newName = item.name
            renaming = item
          }
          if let entry = project.quarantine(of: item.id) {
            Button("Change Quarantine…") {
              quarantining = QuarantineRequest(file: item.id, name: item.name, entry: entry)
            }
            Button("Release from Quarantine") {
              do { try project.releaseFromQuarantine(item.id) } catch { failure = ProjectStore.message(error) }
            }
          } else {
            if let suggestion = project.quarantineSuggestion(for: item.id) {
              Button("Put in Quarantine for Two Weeks (Suggested)") {
                attempt { try project.quarantine(item.id, accepting: suggestion) }
              }
            }
            Button("Quarantine…") { quarantining = QuarantineRequest(file: item.id, name: item.name) }
          }
          Button("Move to Trash") { trash(item) }
          Divider()
        }
        Button("Show in Finder") { project.reveal(item) }
        if project.git.status?.hasCommits == true {
          Button("Show History…") { project.historyPath = item.id }
        }
      }
  }

  private func boardRow(_ name: String) -> some View {
    let isAllTests = name == project.boards.allTests
    return BoardRow(
      name: name, isAllTests: isAllTests,
      run: isAllTests && canRun ? { runAll(project.items, title: project.runAllTitle) } : nil
    )
    .tag(ProjectSidebarSelection.board(name))
    .contextMenu {
      if isAllTests {
        Button("Run All Tests") { runAll(project.items, title: project.runAllTitle) }
          .disabled(!canRun)
        RunModePicker(folder: ".") { failure = $0 }
        Divider()
      }
      Button("Rename…") {
        project.boards.renaming = name
      }
      if !isAllTests {
        Button("Move to Trash") { attempt { try project.boards.moveToTrash(name) } }
      }
      Divider()
      Button("Show in Finder") {
        if let folder = project.boards.folder {
          NSWorkspace.shared.activateFileViewerSelecting([folder.appending(path: name + ProjectBoards.suffix)])
        }
      }
    }
  }

  private func newBoard() {
    attempt {
      let name = try project.boards.create(String(localized: "New Board"))
      project.showBoard(name)
      project.boards.renaming = name
    }
  }

  private var canRun: Bool { !runs.isBusy && !records.isRecording && project.rootURL != nil }

  /// Opens and runs a test; runs every test of a folder as the folder's run mode says.
  private func run(_ item: ProjectItem) {
    guard let root = project.rootURL else { return }
    switch item.kind {
    case .test:
      project.selection = item.id
      if let document = project.document { runs.run(document, in: root) }
    case .folder:
      runAll([item], title: item.name, folder: item.id)
    }
  }

  private func runAll(_ items: [ProjectItem], title: String, folder: String = ".") {
    guard let root = project.rootURL else { return }
    runs.runAll(project.chosen(from: items).map(\.url), in: root, title: title, folder: folder)
  }

  private func trash(_ item: ProjectItem) {
    attempt { try project.moveToTrash(item) }
  }

  private func attempt(_ action: () throws -> Void) {
    do { try action() } catch { failure = error.localizedDescription }
  }
}

/// The sidebar's search. What is typed shows at once; the tests it finds follow once typing
/// pauses, ranked off the main thread, so a project of a thousand tests never holds up a letter.
/// A search started for older text never replaces what a newer one found.
@MainActor
@Observable
final class SidebarSearch {
  /// What is typed in the search field.
  var text = "" {
    didSet { if text != oldValue { search(after: Self.pause) } }
  }
  /// What `found` was found for: the text typed, once its search is done; empty while the
  /// sidebar shows the folders.
  private(set) var query = ""
  /// The tests that match `query`, best first.
  private(set) var found: [ProjectItem] = []
  /// The sidebar lists what was found instead of the folders.
  var isSearching: Bool { !query.isEmpty }

  /// How long typing pauses before the tests are looked through.
  static let pause = Duration.milliseconds(120)

  @ObservationIgnored private var items: [ProjectItem] = []
  @ObservationIgnored private var work: Task<Void, Never>?
  @ObservationIgnored private let rank: @Sendable ([ProjectItem], String) -> [ProjectItem]

  init(rank: @escaping @Sendable ([ProjectItem], String) -> [ProjectItem] = ProjectScanner.tests(in:matching:)) {
    self.rank = rank
  }

  /// Searches `items`, the tests the sidebar lists, from now on; again at once while searching.
  func search(in items: [ProjectItem]) {
    self.items = items
    search(after: .zero)
  }

  private func search(after pause: Duration) {
    work?.cancel()
    let text = text
    guard !text.trimmingCharacters(in: .whitespaces).isEmpty else {
      work = nil
      query = ""
      found = []
      return
    }
    let items = items
    let rank = rank
    work = Task {
      if pause > .zero { try? await Task.sleep(for: pause) }
      guard !Task.isCancelled else { return }
      let found = await Self.ranked(items, text, rank)
      guard !Task.isCancelled else { return }
      self.found = found
      query = text
    }
  }

  @concurrent
  private nonisolated static func ranked(
    _ items: [ProjectItem], _ text: String, _ rank: @Sendable ([ProjectItem], String) -> [ProjectItem]
  ) async -> [ProjectItem] {
    rank(items, text)
  }
}

/// The branch and how many files changed, at the foot of the sidebar; opens Settings › Git. With
/// changes, a Commit button next to it opens the commit window directly.
private struct GitChip: View {
  let status: GitStatus
  @Environment(\.openSettings) private var openSettings
  @Environment(\.openWindow) private var openWindow
  @AppStorage(SettingsTab.key) private var tab = SettingsTab.general

  var body: some View {
    let branch = status.branch ?? String(localized: "No branch")
    HStack(spacing: 8) {
      Button {
        tab = .git
        openSettings()
      } label: {
        HStack(spacing: 6) {
          Image(systemName: "arrow.triangle.branch")
          Text(verbatim: branch)
            .lineLimit(1)
            .truncationMode(.middle)
          if !status.changes.isEmpty {
            Text(verbatim: "· \(status.changes.count)")
              .monospacedDigit()
          }
          Spacer(minLength: 0)
        }
        .foregroundStyle(.secondary)
        .contentShape(.rect)
      }
      .buttonStyle(.plain)
      .help(Text("Commit, pull and push in Settings › Git"))
      .accessibilityLabel(Text("Branch \(branch)"))
      .accessibilityValue(Text(GitText.summary(status.changes)))
      .accessibilityHint(Text("Opens Git settings"))
      if !status.changes.isEmpty {
        Button("Commit…") { openWindow(id: SceneID.commit) }
          .buttonStyle(.borderless)
          .fixedSize()
          .layoutPriority(1)
          .help(Text("Save a version of the changed tests (⌥⌘C)"))
      }
    }
    .font(.callout)
    .padding(.horizontal, 16)
    .padding(.vertical, 8)
  }
}

/// A Run button that shows while the pointer is over the row; VoiceOver offers it as an action.
private struct RunOnHover: ViewModifier {
  let title: Text
  let run: (() -> Void)?
  @State private var isHovering = false

  func body(content: Content) -> some View {
    HStack(spacing: 4) {
      content
      if isHovering, let run {
        Spacer(minLength: 0)
        Button(action: run) {
          Image(systemName: "play.fill")
            .font(.caption2)
            .frame(width: 18, height: 18)
            .contentShape(.rect)
        }
        .buttonStyle(.borderless)
        .help(title)
        .accessibilityHidden(true)
      }
    }
    .onHover { isHovering = $0 }
    .accessibilityAction(named: title) { run?() }
  }
}

/// A board in the sidebar; the board of all tests offers Run All on hover.
private struct BoardRow: View {
  let name: String
  let isAllTests: Bool
  let run: (() -> Void)?

  var body: some View {
    Label(name, systemImage: isAllTests ? "square.grid.3x3.square" : "rectangle.on.rectangle")
      .modifier(RunOnHover(title: Text("Run All Tests"), run: run))
      .help(isAllTests ? Text("Every test of the project, in the frames of their folders") : Text(verbatim: name))
  }
}

private struct SidebarRow: View {
  let item: ProjectItem
  let showsPath: Bool
  /// How the test's last run ended.
  let status: RunStatus?
  let quarantine: Quarantined?
  /// Why the test counts as flaky, when its recent runs say so; a mark in its details also shows.
  let flaky: FlakyEvidence?
  /// Why the test is worth putting in quarantine, when its runs say so.
  let suggestion: QuarantineSuggestion?
  /// Runs the test, or every test of the folder; `nil` while nothing can run.
  let run: (() -> Void)?

  var body: some View {
    switch item.kind {
    case .folder:
      Label(item.name, systemImage: "folder")
        .modifier(RunOnHover(title: Text("Run Folder"), run: run))
        .badge(item.testCount)
        .accessibilityValue(Text("\(item.testCount) tests"))
    case .test:
      Label {
        HStack(spacing: 6) {
          VStack(alignment: .leading, spacing: 1) {
            Text(item.name)
            if showsPath, !item.parentPath.isEmpty {
              Text(item.parentPath)
                .font(.caption)
                .foregroundStyle(.secondary)
            }
          }
          if status != nil || quarantine != nil || flakyHelp != nil || suggestionHelp != nil {
            Spacer(minLength: 0)
          }
          if let quarantine {
            QuarantineMark(entry: quarantine)
              .font(.caption2)
              .accessibilityHidden(true)
          }
          if let suggestionHelp {
            Image(systemName: "bandage")
              .font(.caption2)
              .foregroundStyle(RunStatus.quarantined.color)
              .help(Text(suggestionHelp))
              .accessibilityHidden(true)
          }
          if let flakyHelp, status != .flaky {
            Image(systemName: RunStatus.flaky.dot)
              .font(.caption2)
              .imageScale(.small)
              .foregroundStyle(RunStatus.flaky.color)
              .help(Text(flakyHelp))
              .accessibilityHidden(true)
          }
          if let status {
            Image(systemName: status.dot)
              .font(.caption2)
              .imageScale(.small)
              .foregroundStyle(status.color)
              .help(Text("Last run: \(status.title)"))
              .accessibilityHidden(true)
          }
        }
      } icon: {
        Image(systemName: "flowchart")
      }
      .modifier(RunOnHover(title: Text("Run"), run: run))
      .accessibilityValue(Text(verbatim: accessibilityValue))
      .accessibilityIdentifier(AccessibilityID.Project.test)
    }
  }

  private var flakyHelp: String? { TestFields.flakyHelp(flaky, marked: item.markedFlaky) }

  /// That quarantine is worth it, and where to do it.
  private var suggestionHelp: String? {
    suggestion.map { QuarantineText.suggestion($0) + ". " + String(localized: "Open its menu to put it in quarantine.") }
  }

  private var accessibilityValue: String {
    var parts: [String] = []
    if let quarantine { parts.append(QuarantineText.describe(quarantine)) }
    if let flakyHelp { parts.append(flakyHelp) }
    if let suggestionHelp { parts.append(suggestionHelp) }
    if let status { parts.append(String(localized: "Last run: \(status.title)")) }
    return parts.joined(separator: ". ")
  }
}
