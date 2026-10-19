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
      if let test = healthTest {
        let runs = project.runs.filter { $0.report.file == test }
        if !runs.isEmpty {
          Section {
            HealthView(test: test, health: testHealth(runs: runs.map(\.report), nowMs: Int64(Date.now.timeIntervalSince1970 * 1000)))
          } header: {
            Text("\(runs[0].report.test.isEmpty ? ProjectScanner.testName((test as NSString).lastPathComponent) : runs[0].report.test) · health")
          }
        }
      }
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

  /// The test the health block is about: the one selected among the tests, or else the one of
  /// the selected run.
  private var healthTest: String? {
    if let selection = project.selection, ProjectScanner.isTestFile(selection) { return selection }
    return project.selectedRun.flatMap { id in project.runs.first { $0.id == id }?.report.file }
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
          .frame(width: 22)
          .accessibilityHidden(true)
        VStack(alignment: .leading, spacing: 1) {
          Text(run.report.test.isEmpty ? String(localized: "Untitled Test") : run.report.test)
            .lineLimit(1)
          // Runs started from a phone name it first.
          Text(verbatim: [run.report.requestedBy ?? run.report.schedule.map { String(localized: "Schedule · \($0)") } ?? String(localized: "This Mac"), time, duration].joined(separator: " · "))
            .font(.caption)
            .foregroundStyle(.secondary)
            .lineLimit(1)
        }
      }
      .accessibilityElement(children: .ignore)
      .accessibilityLabel(Text(run.report.test))
      .accessibilityValue(
        run.report.requestedBy.map { Text("\(status.title), started from \($0), \(time), took \(duration)") }
          ?? run.report.schedule.map { Text("\(status.title), started by the schedule \($0), \(time), took \(duration)") }
          ?? Text("\(status.title), \(time), took \(duration)"))
    }
  }
}

/// How a test has been doing: its pass rate over 30 days, its last 20 runs as bars (a pass up,
/// a failure down), where it fails most often and since when it fails.
struct HealthView: View {
  @Environment(ProjectStore.self) private var project
  /// Test file, relative to the project.
  let test: String
  let health: TestHealth

  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      if let percent = health.passPercent {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
          Text(Double(percent) / 100, format: .percent)
            .font(.title2.weight(.semibold))
            .monospacedDigit()
          Text("passed in 30 days")
            .font(.caption)
            .foregroundStyle(.secondary)
        }
        .accessibilityElement(children: .combine)
      } else {
        Text("No runs in the last 30 days")
          .font(.caption)
          .foregroundStyle(.secondary)
      }
      HealthBars(marks: health.recent)
      if let flaky = health.flaky {
        Label {
          Text(RunHistory.flakyEvidence(flaky))
            .foregroundStyle(.secondary)
        } icon: {
          Image(systemName: RunStatus.flaky.symbol)
            .foregroundStyle(RunStatus.flaky.color)
        }
        .font(.caption)
        .lineLimit(4)
        .fixedSize(horizontal: false, vertical: true)
      }
      if let point = health.failsMostAt {
        Text("Fails most often at \(place(point))")
          .font(.caption)
          .foregroundStyle(.secondary)
          .lineLimit(3)
          .fixedSize(horizontal: false, vertical: true)
      }
      if let since = health.failingSince.map(RunHistory.date), since != .distantPast {
        Text(failing(since))
          .font(.caption)
          .foregroundStyle(.red)
          .fixedSize(horizontal: false, vertical: true)
      }
    }
    .padding(.vertical, 4)
  }

  /// The block's type and what it does, and the test it's in when another test runs it.
  private func place(_ point: FailurePoint) -> String {
    let flow: Flow? =
      if point.test == test, project.selection == test {
        project.document?.flow
      } else {
        project.rootURL.flatMap { root in
          (try? String(contentsOf: root.appending(path: point.test), encoding: .utf8)).flatMap { try? loadFlow(json: $0) }
        }
      }
    let block = flow?.blocks.first { $0.id == point.block }
    var text = BlockInfo.title(point.blockType)
    if let block { text += " · " + BlockSummary.text(block.kind, appName: AppCatalog.shared.name(of:)) }
    if point.test != test {
      text += " · " + String(localized: "in \(ProjectScanner.testName((point.test as NSString).lastPathComponent))")
    }
    return text
  }

  private func failing(_ since: Date) -> String {
    let time = since.formatted(date: .omitted, time: .shortened)
    if Calendar.current.isDateInToday(since) { return String(localized: "Started failing today at \(time).") }
    return String(localized: "Failing since \(since.formatted(date: .abbreviated, time: .shortened)).")
  }
}

private struct HealthBars: View {
  let marks: [HealthMark]

  var body: some View {
    HStack(spacing: 2) {
      ForEach(Array(marks.enumerated()), id: \.offset) { _, mark in
        RoundedRectangle(cornerRadius: 2)
          .fill(color(mark))
          .frame(maxWidth: 10)
          .frame(height: 13)
          .offset(y: mark == .failed ? 7 : -7)
      }
      Spacer(minLength: 0)
    }
    .frame(height: 30)
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(Text("Recent runs"))
    .accessibilityValue(Text(verbatim: summary))
  }

  private func color(_ mark: HealthMark) -> Color {
    switch mark {
    case .passed: .green
    case .flaky: .orange
    case .failed: .red
    }
  }

  private var summary: String {
    let flaky = marks.count { $0 == .flaky }
    var parts = [
      String(localized: "Passed: \(marks.count { $0 != .failed })"),
      String(localized: "Failed: \(marks.count { $0 == .failed })"),
    ]
    if flaky > 0 { parts.append(String(localized: "Flaky: \(flaky)")) }
    return parts.joined(separator: " · ")
  }
}
