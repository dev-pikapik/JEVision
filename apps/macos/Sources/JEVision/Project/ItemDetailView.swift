import SwiftUI

/// A folder of tests selected in the sidebar.
struct ItemDetailView: View {
  @Environment(ProjectStore.self) private var project
  let item: ProjectItem

  private var location: String {
    item.parentPath.isEmpty ? project.name : project.name + "/" + item.parentPath
  }

  var body: some View {
    Form {
      Section {
        HStack(spacing: 14) {
          IconTile(symbol: "folder.fill", color: .accentColor, size: 52)
          VStack(alignment: .leading, spacing: 2) {
            Text(item.name)
              .font(.title.bold())
              .textSelection(.enabled)
            Text("In \(location)")
              .foregroundStyle(.secondary)
          }
        }
        .padding(.vertical, 6)
        .accessibilityElement(children: .combine)
      }
      Section {
        LabeledContent("Tests") { Text(item.testCount, format: .number) }
      }
    }
    .formStyle(.grouped)
    .toolbar {
      if project.git.status?.hasCommits == true {
        Button("History", systemImage: "clock.arrow.circlepath") { project.historyPath = item.id }
          .help(Text("Show the commits that changed tests in this folder"))
      }
      Button("Show in Finder", systemImage: "folder") { project.reveal(item) }
        .help(Text("Show in Finder"))
    }
  }
}
