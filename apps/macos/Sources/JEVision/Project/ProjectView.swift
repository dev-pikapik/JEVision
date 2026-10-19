import JEVisionCore
import SwiftUI

struct ProjectView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(RunCenter.self) private var runs
  @Environment(RecordCenter.self) private var records
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @State private var query = ""
  @State private var renaming: ProjectItem?
  @State private var newName = ""
  @State private var newBoardName = ""
  @State private var quarantining: QuarantineRequest?
  @State private var failure: String?
  /// Open folders; the folders around the selected test open by themselves.
  @State private var expanded: Set<ProjectItem.ID> = []

  private var isSearching: Bool { !query.trimmingCharacters(in: .whitespaces).isEmpty }

  var body: some View {
    @Bindable var project = project
    NavigationSplitView {
      Group {
        if project.sidebar == .runs {
          RunsList(query: query)
        } else {
          testList
        }
      }
      // Bars, not insets: the list scrolls under them, and their edge effect keeps them readable.
      .safeAreaBar(edge: .top, spacing: 0) {
        if !project.runs.isEmpty || project.sidebar == .runs {
          Picker("Show", selection: $project.sidebar) {
            Text("Tests").tag(ProjectStore.Sidebar.tests)
            Text("Runs").tag(ProjectStore.Sidebar.runs)
          }
          .pickerStyle(.segmented)
          .labelsHidden()
          .padding(.horizontal, 10)
          .padding(.bottom, 6)
        }
      }
      .safeAreaBar(edge: .bottom, spacing: 0) {
        VStack(spacing: 0) {
          EnvironmentChip()
          ScheduleChip()
          if let status = project.git.status { GitChip(status: status) }
        }
      }
      .searchable(
        text: $query, placement: .sidebar,
        prompt: project.sidebar == .runs ? Text("Filter runs by test") : Text("Search tests"))
      .navigationSplitViewColumnWidth(min: 200, ideal: 250, max: 400)
    } detail: {
      detail
        .overlay(alignment: .bottom) {
          if let result = runs.batchResult {
            BatchResultBar(result: result)
              .transition(reduceMotion ? .opacity : .move(edge: .bottom).combined(with: .opacity))
          }
        }
        .animation(reduceMotion ? nil : .snappy, value: runs.batchResult)
    }
    .onChange(of: runs.result) { _, result in
      // Run Again from a report: the new run's report takes its place.
      if project.sidebar == .runs, project.selectedRun != nil, let run = result?.run { project.showRun(run) }
    }
    .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
      project.git.refresh()
    }
    .quarantineAlert($quarantining)
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
    if project.sidebar == .runs {
      if let dir = project.selectedRun {
        RunReportView(dir: dir)
          .id(dir)
      } else {
        ContentUnavailableView(
          "Select a run", systemImage: "clock.arrow.circlepath",
          description: Text("Its report shows every step, with screenshots."))
      }
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
    let found = isSearching ? ProjectScanner.tests(in: project.items, matching: query) : []
    let selection = Binding<ProjectItem.ID?>(
      get: { project.showsBoard ? project.boards.current.map(Self.boardTag) : project.selection },
      set: { id in
        if let id, id.hasPrefix(Self.boardTag("")) {
          project.showBoard(String(id.dropFirst(Self.boardTag("").count)))
        } else {
          project.selection = id
        }
      })
    return List(selection: selection) {
        if isSearching {
          ForEach(found) { row($0, showsPath: true) }
        } else {
          Section {
            ForEach(project.boards.names, id: \.self) { boardRow($0) }
          } header: {
            HStack {
              Text("Boards")
              Spacer()
              Button("New Board", systemImage: "plus") { newBoard() }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .help(Text("Make a new board"))
            }
          }
          Section("Tests") {
            ForEach(project.items) { tree($0) }
          }
        }
      }
      // A fresh list for search results: reusing the tree's rows left a ghost of the selected one.
      .id(isSearching)
      .onChange(of: project.selection, initial: true) {
        guard let id = project.selection else { return }
        var folder = ""
        for part in id.split(separator: "/").dropLast() {
          folder = folder.isEmpty ? String(part) : folder + "/" + part
          expanded.insert(folder)
        }
      }
      .overlay {
        if isSearching, found.isEmpty {
          ContentUnavailableView.search(text: query)
        }
      }
      .onDeleteCommand {
        if let id = project.selection, let item = project.items.item(withID: id), item.kind == .test { trash(item) }
      }
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
      flaky: project.flaky[item.id], run: canRun ? { run(item) } : nil
    )
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
          if project.quarantine(of: item.id) != nil {
            Button("Release from Quarantine") {
              do { try project.releaseFromQuarantine(item.id) } catch { failure = ProjectStore.message(error) }
            }
          } else {
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

  /// Tag of a board's row: no test or folder path looks like it.
  private static func boardTag(_ name: String) -> String { "\u{1}board:" + name }

  private func boardRow(_ name: String) -> some View {
    let isAllTests = name == project.boards.allTests
    return BoardRow(
      name: name, isAllTests: isAllTests,
      run: isAllTests && canRun ? { runAll(project.items, title: String(localized: "All tests")) } : nil
    )
    .tag(Self.boardTag(name))
    .contextMenu {
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

  private var canRun: Bool { !runs.isRunning && !records.isRecording && project.rootURL != nil }

  /// Opens and runs a test; runs every test of a folder one after another.
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
    runs.runAll(items.flatMap(\.tests).map(\.url), in: root, title: title, folder: folder)
  }

  private func trash(_ item: ProjectItem) {
    attempt { try project.moveToTrash(item) }
  }

  private func attempt(_ action: () throws -> Void) {
    do { try action() } catch { failure = error.localizedDescription }
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
            .font(.system(size: 9))
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
  /// Why the test counts as flaky, when its recent runs say so.
  let flaky: FlakyEvidence?
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
          if status != nil || quarantine != nil || flaky != nil {
            Spacer(minLength: 0)
          }
          if let quarantine {
            QuarantineMark(entry: quarantine)
              .font(.system(size: 9))
              .accessibilityHidden(true)
          }
          if let flaky, status != .flaky {
            Image(systemName: RunStatus.flaky.dot)
              .font(.system(size: 7))
              .foregroundStyle(RunStatus.flaky.color)
              .help(Text(RunHistory.flakyEvidence(flaky)))
              .accessibilityHidden(true)
          }
          if let status {
            Image(systemName: status.dot)
              .font(.system(size: 7))
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
    }
  }

  private var accessibilityValue: String {
    var parts: [String] = []
    if let quarantine { parts.append(QuarantineText.describe(quarantine)) }
    if let flaky { parts.append(RunHistory.flakyEvidence(flaky)) }
    if let status { parts.append(String(localized: "Last run: \(status.title)")) }
    return parts.joined(separator: ". ")
  }
}
