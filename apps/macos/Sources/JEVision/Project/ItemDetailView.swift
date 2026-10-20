import JEVisionCore
import SwiftUI

/// A folder of tests selected in the sidebar.
struct ItemDetailView: View {
  @Environment(ProjectStore.self) private var project
  let item: ProjectItem
  @State private var notes: [FolderNote] = []

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
      if !notes.isEmpty {
        Section("Notes") {
          ForEach(notes, id: \.note.id) { entry in
            FolderNoteRow(entry: entry) { project.selection = entry.test }
          }
        }
      }
    }
    .formStyle(.grouped)
    .task(id: item.id) {
      guard let root = project.rootURL?.path(percentEncoded: false) else { return }
      let folder = item.id
      notes = await Task.detached { folderNotes(project: root, folder: folder) }.value
    }
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

/// A note a test wrote about the folder: who wrote it, the test it is in and the text.
private struct FolderNoteRow: View {
  let entry: FolderNote
  let open: () -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      HStack(spacing: 8) {
        if !entry.note.author.isEmpty { Initials(name: entry.note.author) }
        VStack(alignment: .leading, spacing: 0) {
          Text(entry.note.author.isEmpty ? String(localized: "Someone") : entry.note.author).font(.callout.bold())
          if let date = NoteTime.date(entry.note.time) {
            Text(date, format: .relative(presentation: .named)).font(.caption).foregroundStyle(.secondary)
          }
        }
        Spacer(minLength: 0)
        Button("Open \(entry.name)", action: open)
          .buttonStyle(.link)
          .help(Text("Open the test that has this note"))
      }
      MarkdownText(text: entry.note.text)
        .textSelection(.enabled)
    }
    .padding(.vertical, 2)
    .accessibilityElement(children: .contain)
  }
}
