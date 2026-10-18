import JEVisionCore
import SwiftUI

struct ProjectView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(RunCenter.self) private var runs
  @Environment(RecordCenter.self) private var records
  @State private var query = ""
  @State private var renaming: ProjectItem?
  @State private var newName = ""
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
      .safeAreaInset(edge: .top, spacing: 0) {
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
      .safeAreaInset(edge: .bottom, spacing: 0) {
        VStack(spacing: 0) {
          EnvironmentChip()
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
              .transition(.move(edge: .bottom).combined(with: .opacity))
          }
        }
        .animation(.snappy, value: runs.batchResult)
    }
    .onChange(of: runs.result) { _, result in
      // Run Again from a report: the new run's report takes its place.
      if project.sidebar == .runs, project.selectedRun != nil, let run = result?.run { project.showRun(run) }
    }
    .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
      project.git.refresh()
    }
    .alert("Rename Test", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
      TextField("Name", text: $newName)
      Button("Rename") {
        guard let item = renaming else { return }
        attempt { try project.rename(item, to: newName) }
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
    } else if project.showsMap {
      ProjectMapView()
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
    let latest = RunHistory.latest(project.runs)
    let selection = Binding<ProjectItem.ID?>(
      get: { project.showsMap ? Self.mapTag : project.selection },
      set: { id in
        if id == Self.mapTag {
          project.showsMap = true
          project.selection = nil
        } else {
          project.selection = id
        }
      })
    return List(selection: selection) {
        if isSearching {
          ForEach(ProjectScanner.tests(in: project.items, matching: query)) { row($0, showsPath: true, latest: latest) }
        } else {
          MapRow(run: canRun ? { runAll(project.items, title: String(localized: "All tests")) } : nil)
            .tag(Self.mapTag)
          Section("Tests") {
            ForEach(project.items) { tree($0, latest: latest) }
          }
        }
      }
      .onChange(of: project.selection, initial: true) {
        guard let id = project.selection else { return }
        var folder = ""
        for part in id.split(separator: "/").dropLast() {
          folder = folder.isEmpty ? String(part) : folder + "/" + part
          expanded.insert(folder)
        }
      }
      .overlay {
        if isSearching, ProjectScanner.tests(in: project.items, matching: query).isEmpty {
          ContentUnavailableView.search(text: query)
        }
      }
      .onDeleteCommand {
        if let id = project.selection, let item = project.items.item(withID: id), item.kind == .test { trash(item) }
      }
  }

  private func tree(_ item: ProjectItem, latest: [String: RunStatus]) -> AnyView {
    guard let children = item.outlineChildren else { return AnyView(row(item, showsPath: false, latest: latest)) }
    let isExpanded = Binding(
      get: { expanded.contains(item.id) },
      set: { if $0 { expanded.insert(item.id) } else { expanded.remove(item.id) } })
    return AnyView(
      DisclosureGroup(isExpanded: isExpanded) {
        ForEach(children) { tree($0, latest: latest) }
      } label: {
        row(item, showsPath: false, latest: latest)
      })
  }

  private func row(_ item: ProjectItem, showsPath: Bool, latest: [String: RunStatus]) -> some View {
    SidebarRow(item: item, showsPath: showsPath, status: latest[item.id], run: canRun ? { run(item) } : nil)
      .contextMenu {
        Button(item.kind == .test ? "Run" : "Run Folder") { run(item) }
          .disabled(!canRun)
        Divider()
        if item.kind == .test {
          Button("Rename…") {
            newName = item.name
            renaming = item
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

  /// Tag of the Project Map row: no test or folder path looks like it.
  private static let mapTag = "\u{1}map"

  private var canRun: Bool { !runs.isRunning && !records.isRecording && project.rootURL != nil }

  /// Opens and runs a test; runs every test of a folder one after another.
  private func run(_ item: ProjectItem) {
    guard let root = project.rootURL else { return }
    switch item.kind {
    case .test:
      project.selection = item.id
      if let document = project.document { runs.run(document, in: root) }
    case .folder:
      runAll([item], title: item.name)
    }
  }

  private func runAll(_ items: [ProjectItem], title: String) {
    guard let root = project.rootURL else { return }
    runs.runAll(items.flatMap(\.tests).map(\.url), in: root, title: title)
  }

  private func trash(_ item: ProjectItem) {
    attempt { try project.moveToTrash(item) }
  }

  private func attempt(_ action: () throws -> Void) {
    do { try action() } catch { failure = error.localizedDescription }
  }
}

/// The branch and how many files changed, at the foot of the sidebar; opens Settings › Git.
private struct GitChip: View {
  let status: GitStatus
  @Environment(\.openSettings) private var openSettings
  @AppStorage(SettingsTab.key) private var tab = SettingsTab.general

  var body: some View {
    let branch = status.branch ?? String(localized: "No branch")
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
      .font(.callout)
      .foregroundStyle(.secondary)
      .padding(.horizontal, 16)
      .padding(.vertical, 8)
      .contentShape(.rect)
    }
    .buttonStyle(.plain)
    .help(Text("Commit, pull and push in Settings › Git"))
    .accessibilityLabel(Text("Branch \(branch)"))
    .accessibilityValue(Text(GitText.summary(status.changes)))
    .accessibilityHint(Text("Opens Git settings"))
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

/// The first row of the test list: the project map, with Run All on hover.
private struct MapRow: View {
  let run: (() -> Void)?

  var body: some View {
    Label("Project Map", systemImage: "rectangle.3.group")
      .modifier(RunOnHover(title: Text("Run All Tests"), run: run))
  }
}

private struct SidebarRow: View {
  let item: ProjectItem
  let showsPath: Bool
  /// How the test's last run ended.
  let status: RunStatus?
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
          if let status {
            Spacer(minLength: 0)
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
      .accessibilityValue(status.map { Text("Last run: \($0.title)") } ?? Text(verbatim: ""))
    }
  }
}
