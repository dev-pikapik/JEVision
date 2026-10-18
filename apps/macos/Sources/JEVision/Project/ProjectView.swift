import JEVisionCore
import SwiftUI

struct ProjectView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(RunCenter.self) private var runs
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
        if let status = project.git.status { GitChip(status: status) }
      }
      .searchable(
        text: $query, placement: .sidebar,
        prompt: project.sidebar == .runs ? Text("Filter runs by test") : Text("Search tests"))
      .navigationSplitViewColumnWidth(min: 200, ideal: 250, max: 400)
    } detail: {
      if project.sidebar == .runs {
        if let dir = project.selectedRun {
          RunReportView(dir: dir)
            .id(dir)
        } else {
          ContentUnavailableView(
            "Select a run", systemImage: "clock.arrow.circlepath",
            description: Text("Its report shows every step, with screenshots."))
        }
      } else if let document = project.document {
        EditorView(document: document)
          .id(document.url)
      } else if let id = project.selection, let item = project.items.item(withID: id) {
        ItemDetailView(item: item)
      } else {
        ContentUnavailableView("Select a test", systemImage: "flowchart")
      }
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

  private var testList: some View {
    @Bindable var project = project
    let latest = RunHistory.latest(project.runs)
    return List(selection: $project.selection) {
        if isSearching {
          ForEach(ProjectScanner.tests(in: project.items, matching: query)) { row($0, showsPath: true, latest: latest) }
        } else {
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
    SidebarRow(item: item, showsPath: showsPath, status: latest[item.id])
      .contextMenu {
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

private struct SidebarRow: View {
  let item: ProjectItem
  let showsPath: Bool
  /// How the test's last run ended.
  let status: RunStatus?

  var body: some View {
    switch item.kind {
    case .folder:
      Label(item.name, systemImage: "folder")
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
      .accessibilityValue(status.map { Text("Last run: \($0.title)") } ?? Text(verbatim: ""))
    }
  }
}
