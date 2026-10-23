import JEVisionCore
import SwiftUI

/// Settings › Advanced: naming the project's files by labels, which stay the same when a file is
/// moved or renamed, and the copies of the tests JEVision keeps before it changes their format.
struct FileFormatSection: View {
  @Environment(ProjectStore.self) private var project
  /// What using labels would change, waiting for the person to confirm.
  @State private var preview: MigrationReport?
  /// What the last change did.
  @State private var result: MigrationReport?
  @State private var backups: [Backup] = []
  /// The copy to put back, waiting for the person to confirm.
  @State private var restoring: Backup?
  @State private var restored: Int?
  /// Why using labels changed nothing.
  @State private var note: String?
  @State private var failure: String?

  var body: some View {
    Section {
      if let root = project.rootURL {
        LabeledContent {
          Button("Use File Labels…") { check(root) }
        } label: {
          RowLabel(
            Text("Use file labels in this project"),
            Text("Tests then find their pictures and tables even after the files are moved or renamed. Colleagues need this version of JEVision or newer to open the tests."))
        }
        .settingAnchor("fileLabels")
        if let note {
          Text(note)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        }
        if let result {
          Label(done(result), systemImage: "checkmark.circle.fill")
            .foregroundStyle(.green)
            .fixedSize(horizontal: false, vertical: true)
        }
        if let restored {
          Label(String(localized: "\(restored) files restored. The tests as they were just before are backed up too."), systemImage: "checkmark.circle.fill")
            .foregroundStyle(.green)
            .fixedSize(horizontal: false, vertical: true)
        }
        if backups.isEmpty {
          LabeledContent {
            Text("None")
          } label: {
            RowLabel(Text("Backups of the tests"), Text("JEVision makes one each time it changes the format of tests."))
          }
          .settingAnchor("backups")
        } else {
          ForEach(Array(backups.enumerated()), id: \.element.id) { index, backup in
            LabeledContent {
              Button("Restore…") { restoring = backup }
            } label: {
              RowLabel(Text(Self.date(backup)), Text("\(backup.files.count) files"))
            }
            .settingAnchor(index == 0 ? "backups" : "backup-\(backup.id)")
          }
        }
      } else {
        LabeledContent {
          Button("Open Project…", action: project.choose)
        } label: {
          RowLabel(Text("No project is open"), Text("Open the folder with your tests to change the format of its files."))
        }
        .settingAnchor("fileLabels")
      }
    } header: {
      Text("Project file format")
    } footer: {
      Text("Before JEVision changes the format of tests, it backs them up in the project, so you can go back.")
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
    }
    .task(id: project.rootURL) { reloadBackups() }
    .alert(
      "Use file labels in this project?", isPresented: Binding(get: { preview != nil }, set: { if !$0 { preview = nil } }),
      presenting: preview
    ) { _ in
      Button("Use File Labels") { migrate() }
      Button("Cancel", role: .cancel) {}
    } message: { preview in
      Text(Self.summary(preview))
    }
    .alert(
      "Restore the tests from this backup?",
      isPresented: Binding(get: { restoring != nil }, set: { if !$0 { restoring = nil } }), presenting: restoring
    ) { backup in
      Button("Restore", role: .destructive) { restore(backup) }
      Button("Cancel", role: .cancel) {}
    } message: { backup in
      Text("\(backup.files.count) files go back to how they were on \(Self.date(backup)). The tests as they are now are backed up first, so you can return to them.")
    }
    .alert(
      "Can’t change the project files", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } }),
      presenting: failure
    ) { _ in
      Button("OK") {}
    } message: { Text($0) }
  }

  /// What a dry run found, as the question before the change.
  static func summary(_ report: MigrationReport) -> String {
    let unreadable = report.problems.count
    guard !report.files.isEmpty else {
      return unreadable == 0
        ? String(localized: "There’s nothing to change: no test in this project names a file by its path.")
        : String(localized: "There’s nothing to change. Tests that can’t be read and stay as they are: \(unreadable)")
    }
    var lines = [
      String(localized: "Tests to change: \(report.files.count)"),
      String(localized: "Paths replaced by labels: \(report.files.map(\.labels.count).reduce(0, +))"),
      String(localized: "Files to get a label: \(report.files.map(\.newSidecars.count).reduce(0, +))"),
    ]
    if unreadable > 0 { lines.append(String(localized: "Tests that can’t be read and stay as they are: \(unreadable)")) }
    lines.append(String(localized: "Before the change, JEVision backs up these tests in the project."))
    return lines.joined(separator: "\n")
  }

  static func date(_ backup: Backup) -> String {
    (try? Date(backup.created, strategy: .iso8601))?.formatted(date: .abbreviated, time: .shortened) ?? backup.created
  }

  private func done(_ report: MigrationReport) -> String {
    guard let id = report.backup, let backup = backups.first(where: { $0.id == id }) else {
      return String(localized: "Tests changed: \(report.files.count)")
    }
    return String(localized: "Tests changed: \(report.files.count). The backup from before the change is the one of \(Self.date(backup)) below.")
  }

  private func check(_ root: URL) {
    (result, restored, note) = (nil, nil, nil)
    do {
      let report = try migrateProject(project: root.path(percentEncoded: false), dryRun: true)
      if report.files.isEmpty {
        note = Self.summary(report)
      } else {
        preview = report
      }
    } catch {
      failure = FileLabels.message(error)
    }
  }

  private func migrate() {
    guard let root = project.rootURL else { return }
    do {
      let report = try migrateProject(project: root.path(percentEncoded: false), dryRun: false)
      FileLabels.setKeepsPlainPaths(false, in: root)
      reloadBackups()
      result = report
      for document in project.openDocuments { document.reload() }
    } catch {
      failure = FileLabels.message(error)
    }
  }

  private func restore(_ backup: Backup) {
    guard let root = project.rootURL else { return }
    (result, note) = (nil, nil)
    do {
      restored = try restoreBackup(project: root.path(percentEncoded: false), id: backup.id).count
      for document in project.openDocuments { document.reload() }
    } catch {
      failure = FileLabels.message(error)
    }
    reloadBackups()
  }

  private func reloadBackups() {
    backups = project.rootURL.map { listBackups(project: $0.path(percentEncoded: false)) } ?? []
  }
}
