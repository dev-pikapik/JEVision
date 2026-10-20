import SwiftUI

/// The projects open in JEVision on one Mac.
struct ProjectsView: View {
  let host: HostRecord
  @Environment(AppModel.self) private var model
  @State private var projects: Loadable<[Project]> = .loading

  var body: some View {
    LoadableView(state: projects, retry: load) { projects in
      if projects.isEmpty {
        ContentUnavailableView(
          "No Projects Yet", systemImage: "folder",
          description: Text("Open a project in JEVision on the Mac, and it shows up here."))
      } else {
        List(projects) { project in
          NavigationLink(value: Route.tests(host, project: project.name, folder: "")) {
            Label {
              Text(project.name)
            } icon: {
              IconTile(symbol: "folder.fill", color: .blue)
            }
          }
        }
        .refreshable { await load() }
      }
    }
    .navigationTitle(host.name)
    .task { await load() }
  }

  private func load() async {
    if projects.value == nil { projects = .loading }
    projects = await model.fetch(host) { session throws(RemoteError) in try await session.listProjects() }
  }
}

/// One folder of a project's tests.
struct TestsView: View {
  let host: HostRecord
  let project: String
  let folder: String
  @Environment(AppModel.self) private var model
  @State private var entries: Loadable<[TestEntry]> = .loading

  var body: some View {
    LoadableView(state: entries, retry: load) { entries in
      let children = TestTree.children(of: folder, in: entries)
      if children.isEmpty {
        ContentUnavailableView(
          "No Tests Here", systemImage: "doc.text",
          description: Text("Tests you make in JEVision on the Mac show up here."))
      } else {
        List(children) { entry in
          if entry.isFolder {
            NavigationLink(value: Route.tests(host, project: project, folder: entry.path)) {
              // Laid out like a test's row, so names line up at every text size.
              HStack(spacing: 12) {
                IconTile(symbol: "folder.fill", color: .gray)
                Text(entry.name)
              }
            }
          } else {
            NavigationLink(value: Route.test(host, project: project, path: entry.path)) {
              TestRow(entry: entry)
            }
          }
        }
        .refreshable { await load() }
      }
    }
    .navigationTitle(folder.isEmpty ? project : String(folder.split(separator: "/").last ?? ""))
    .toolbar {
      ToolbarItem(placement: .topBarTrailing) {
        NavigationLink(value: Route.runs(host, project: project)) {
          Label("Runs", systemImage: "clock.arrow.circlepath")
        }
      }
    }
    .task { await load() }
  }

  private func load() async {
    if entries.value == nil { entries = .loading }
    let project = project
    entries = await model.fetch(host) { session throws(RemoteError) in try await session.listTests(project: project) }
  }
}

private struct TestRow: View {
  let entry: TestEntry

  var body: some View {
    HStack(spacing: 12) {
      IconTile(symbol: "doc.text.fill", color: .indigo)
      VStack(alignment: .leading, spacing: 2) {
        Text(entry.name)
        if let last = entry.lastRun {
          let style = VerdictStyle(last.verdict)
          HStack(spacing: 4) {
            style.image.imageScale(.small)
            Text("\(style.title) · \(last.startedAt, format: .relative(presentation: .named))")
          }
          .font(.subheadline)
          .foregroundStyle(.secondary)
        } else {
          Text("Not run yet")
            .font(.subheadline)
            .foregroundStyle(.secondary)
        }
      }
    }
    .accessibilityElement(children: .combine)
  }
}
