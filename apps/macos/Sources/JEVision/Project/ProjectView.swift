import SwiftUI

struct ProjectView: View {
  @Environment(ProjectStore.self) private var project
  @State private var query = ""
  @State private var renaming: ProjectItem?
  @State private var newName = ""
  @State private var failure: String?

  private var isSearching: Bool { !query.trimmingCharacters(in: .whitespaces).isEmpty }

  var body: some View {
    @Bindable var project = project
    NavigationSplitView {
      List(selection: $project.selection) {
        if isSearching {
          ForEach(ProjectScanner.tests(in: project.items, matching: query)) { row($0, showsPath: true) }
        } else {
          Section("Tests") {
            OutlineGroup(project.items, children: \.outlineChildren) { row($0, showsPath: false) }
          }
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
      .searchable(text: $query, placement: .sidebar, prompt: Text("Search tests"))
      .navigationSplitViewColumnWidth(min: 200, ideal: 250, max: 400)
    } detail: {
      if let document = project.document {
        EditorView(document: document)
          .id(document.url)
      } else if let id = project.selection, let item = project.items.item(withID: id) {
        ItemDetailView(item: item)
      } else {
        ContentUnavailableView("Select a test", systemImage: "flowchart")
      }
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

  private func row(_ item: ProjectItem, showsPath: Bool) -> some View {
    SidebarRow(item: item, showsPath: showsPath)
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
      }
  }

  private func trash(_ item: ProjectItem) {
    attempt { try project.moveToTrash(item) }
  }

  private func attempt(_ action: () throws -> Void) {
    do { try action() } catch { failure = error.localizedDescription }
  }
}

private struct SidebarRow: View {
  let item: ProjectItem
  let showsPath: Bool

  var body: some View {
    switch item.kind {
    case .folder:
      Label(item.name, systemImage: "folder")
        .badge(item.testCount)
        .accessibilityValue(Text("\(item.testCount) tests"))
    case .test:
      Label {
        VStack(alignment: .leading, spacing: 1) {
          Text(item.name)
          if showsPath, !item.parentPath.isEmpty {
            Text(item.parentPath)
              .font(.caption)
              .foregroundStyle(.secondary)
          }
        }
      } icon: {
        Image(systemName: "flowchart")
      }
    }
  }
}
