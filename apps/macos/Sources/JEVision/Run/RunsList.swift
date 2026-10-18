import JEVisionCore
import SwiftUI

/// The project's run history in the sidebar, newest first, by day.
struct RunsList: View {
  @Environment(ProjectStore.self) private var project
  /// Shows only runs of tests whose name contains it.
  let query: String
  @State private var failure: String?

  var body: some View {
    @Bindable var project = project
    let days = RunHistory.days(project.runs, matching: query)
    List(selection: $project.selectedRun) {
      ForEach(days) { day in
        Section(RunHistory.title(of: day.day)) {
          ForEach(day.runs) { run in
            RunRow(run: run, isToday: Calendar.current.isDateInToday(day.day))
              .tag(run.id)
              .contextMenu {
                Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([run.url]) }
                Divider()
                Button("Move to Trash") { trash(run) }
              }
          }
        }
      }
    }
    .overlay {
      if days.isEmpty, !query.trimmingCharacters(in: .whitespaces).isEmpty {
        ContentUnavailableView.search(text: query)
      }
    }
    .onDeleteCommand {
      if let id = project.selectedRun, let run = project.runs.first(where: { $0.id == id }) { trash(run) }
    }
    .alert(
      "Something went wrong", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } }),
      presenting: failure
    ) { _ in
      Button("OK") {}
    } message: { Text($0) }
  }

  private func trash(_ run: SavedRun) {
    do { try project.moveToTrash(run) } catch { failure = error.localizedDescription }
  }
}

private struct RunRow: View {
  let run: SavedRun
  let isToday: Bool

  var body: some View {
    let status = run.status
    // Today's runs say how long ago they were, and keep saying it as time passes.
    TimelineView(.everyMinute) { _ in
      let time = isToday
        ? run.startedAt.formatted(.relative(presentation: .named, unitsStyle: .abbreviated))
        : run.startedAt.formatted(date: .omitted, time: .shortened)
      let duration = RunHistory.duration(run.report.durationMs)
      HStack(spacing: 8) {
        Image(systemName: status.symbol)
          .foregroundStyle(status.color)
          .imageScale(.large)
          .accessibilityHidden(true)
        VStack(alignment: .leading, spacing: 1) {
          Text(run.report.test.isEmpty ? String(localized: "Untitled Test") : run.report.test)
            .lineLimit(1)
          // Runs started from a phone name it first.
          Text(verbatim: [run.report.requestedBy, time, duration].compactMap { $0 }.joined(separator: " · "))
            .font(.caption)
            .foregroundStyle(.secondary)
            .lineLimit(1)
        }
      }
      .accessibilityElement(children: .ignore)
      .accessibilityLabel(Text(run.report.test))
      .accessibilityValue(
        run.report.requestedBy.map { Text("\(status.title), started from \($0), \(time), took \(duration)") }
          ?? Text("\(status.title), \(time), took \(duration)"))
    }
  }
}
