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
    let days = RunHistory.days(project.runs, matching: query, from: project.runSource)
    let isFiltering = !query.trimmingCharacters(in: .whitespaces).isEmpty
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
    .safeAreaBar(edge: .top, spacing: 0) { RunSourceBar() }
    // A report of a run the new filter hides would stay open beside a list that lacks it.
    .onChange(of: project.runSource) {
      project.selectedRun = RunHistory.selection(project.selectedRun, in: project.runs, from: project.runSource)
    }
    // Opening the runs asks GitHub for the ones CI finished since last time.
    .task { project.ci.refresh() }
    .overlay {
      if days.isEmpty, !isFiltering, project.runSource == .ci {
        CIRunsUnavailable()
      } else if project.runs.isEmpty {
        ContentUnavailableView {
          Label("No runs yet", systemImage: "clock.arrow.circlepath")
        } description: {
          Text("Every run of a test is kept here.")
        }
      } else if days.isEmpty, isFiltering {
        ContentUnavailableView.search(text: query)
      } else if days.isEmpty, let source = project.runSource {
        ContentUnavailableView {
          Label("No runs from \(source.title)", systemImage: source.symbol)
        } description: {
          Text("Pick another source to see the rest.")
        }
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
          // Runs started from a phone name it first; those from CI, the pull request or branch.
          Text(verbatim: [RunHistory.origin(run.report), time, duration].joined(separator: " · "))
            .font(.caption)
            .foregroundStyle(.secondary)
            .lineLimit(1)
        }
      }
      .accessibilityElement(children: .ignore)
      .accessibilityLabel(Text(run.report.test))
      .accessibilityValue(
        run.report.ci.map { _ in Text("\(status.title), from \(RunHistory.origin(run.report)), \(time), took \(duration)") }
          ?? run.report.requestedBy.map { Text("\(status.title), started from \($0), \(time), took \(duration)") }
          ?? run.report.schedule.map { Text("\(status.title), started by the schedule \($0), \(time), took \(duration)") }
          ?? Text("\(status.title), \(time), took \(duration)"))
    }
  }
}

/// Above the runs: which of them to show, and a button to fetch what CI has finished.
private struct RunSourceBar: View {
  @Environment(ProjectStore.self) private var project

  var body: some View {
    @Bindable var project = project
    let ci = project.ci
    VStack(alignment: .leading, spacing: 4) {
      HStack(spacing: 6) {
        Picker("Started from", selection: $project.runSource) {
          Text("All Runs").tag(RunSource?.none)
          Divider()
          ForEach(RunSource.allCases) { source in
            Label(source.title, systemImage: source.symbol).tag(RunSource?.some(source))
          }
        }
        .labelsHidden()
        .pickerStyle(.menu)
        .help(Text("Show only the runs started from this Mac, by a schedule, from an iPhone or by CI"))
        Spacer(minLength: 0)
        if ci.state != .notOnGitHub {
          Button {
            ci.refresh()
          } label: {
            if ci.state == .checking {
              ProgressView().controlSize(.small)
            } else {
              Image(systemName: "arrow.triangle.2.circlepath")
            }
          }
          .buttonStyle(.borderless)
          .disabled(ci.state == .checking)
          .help(Text("Fetch the runs CI has finished on GitHub"))
          .accessibilityLabel(Text("Refresh CI Runs"))
        }
      }
      if let problem = ProjectCI.problem(in: ci.state) {
        Text(problem)
          .font(.caption)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
    }
    .padding(.horizontal, 10)
    .padding(.bottom, 6)
  }
}

/// Shown for the CI filter when no CI run is in the history: why, and what to do about it.
private struct CIRunsUnavailable: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.openSettings) private var openSettings

  var body: some View {
    switch project.ci.state {
    case .idle, .checking:
      ProgressView("Fetching CI runs…")
    case .needsSignIn:
      ContentUnavailableView {
        Label("Sign In to See CI Runs", systemImage: "person.crop.circle.badge.questionmark")
      } description: {
        Text("CI runs come from GitHub. Sign in to GitHub in Settings, and the runs of this project’s workflow show up here.")
      } actions: {
        Button("Open GitHub Settings") { show(.git) }
      }
    case .notOnGitHub:
      ContentUnavailableView {
        Label("This project isn’t on GitHub", systemImage: "gearshape.2")
      } description: {
        Text("CI runs come from GitHub Actions. Connect the project’s repository to GitHub to see them here.")
      }
    case .done(let found) where found.artifacts == 0:
      ContentUnavailableView {
        Label("No CI Runs Yet", systemImage: "gearshape.2")
      } description: {
        Text("The workflow hasn’t uploaded any reports. Add it to the project in Settings (replace an older one), push it, and let it run.")
      } actions: {
        Button("Open Advanced Settings") { show(.advanced) }
      }
    case .done:
      ContentUnavailableView {
        Label("No CI Runs to Show", systemImage: "gearshape.2")
      } description: {
        Text("CI has uploaded reports, but none had a run JEVision can read.")
      }
    case .failed(let message):
      ContentUnavailableView {
        Label("Couldn’t Get CI Runs", systemImage: "exclamationmark.triangle")
      } description: {
        Text(message)
      } actions: {
        Button("Try Again") { project.ci.refresh() }
      }
    }
  }

  private func show(_ tab: SettingsTab) {
    UserDefaults.standard.set(tab.rawValue, forKey: SettingsTab.key)
    openSettings()
  }
}

/// How a test has been doing: its pass rate over 30 days, its last 20 runs as bars (a pass up,
/// a failure down), where it fails most often and since when it fails.
struct HealthView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.openWindow) private var openWindow
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
        if health.passedOnRetry > 0 {
          Text("\(Int(health.passedOnRetry)) passed on a retry")
            .font(.caption)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        }
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
        .fixedSize(horizontal: false, vertical: true)
      }
      if let point = health.failsMostAt {
        Text("Fails most often at \(place(point))")
          .font(.caption)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
      if let since = health.failingSince.map(RunHistory.date), since != .distantPast {
        Text(failing(since))
          .font(.caption)
          .foregroundStyle(.red)
          .fixedSize(horizontal: false, vertical: true)
      }
      Button("All-Time Statistics…", systemImage: "chart.bar.xaxis") {
        project.statisticsTest = test
        openWindow(id: SceneID.statistics)
      }
      .buttonStyle(.link)
      .font(.caption)
      .help(Text("Pass rate, durations, trend and where it fails, over every run of the test"))
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
      text += " · " + String(localized: "in \(ProjectScanner.testName(path: point.test))")
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
        Image(systemName: mark.status.dot)
          .foregroundStyle(mark.status.color)
          .font(.caption)
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
