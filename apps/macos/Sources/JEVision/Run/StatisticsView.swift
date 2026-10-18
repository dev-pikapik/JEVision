import Charts
import JEVisionCore
import SwiftUI

/// How a test has done over its whole life, or which tests of the project fail most and take
/// longest. The core counts; this only draws.
struct StatisticsView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.openWindow) private var openWindow
  @State private var shown: Shown?

  /// What was counted, and from which runs.
  enum Shown {
    case test(String, TestStatistics, runs: [SavedRun])
    case project(ProjectStatistics)
  }

  var body: some View {
    @Bindable var project = project
    Group {
      switch shown {
      case .test(_, let statistics, let runs): TestStatisticsView(statistics: statistics, runs: runs, open: open)
      case .project(let statistics): ProjectStatisticsView(statistics: statistics) { project.statisticsTest = $0 }
      case nil: ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
      }
    }
    .toolbar {
      ToolbarItem(placement: .principal) {
        Picker("Statistics of", selection: $project.statisticsTest) {
          Text("Whole Project").tag(String?.none)
          Divider()
          ForEach(tests, id: \.file) { test in
            Text(verbatim: test.name).tag(Optional(test.file))
          }
        }
        .fixedSize()
      }
    }
    .navigationTitle(Text("Statistics"))
    .task(id: Key(test: project.statisticsTest, runs: project.runs.count, newest: project.runs.first?.id)) {
      await count()
    }
  }

  /// What the statistics depend on.
  private struct Key: Equatable {
    let test: String?
    let runs: Int
    let newest: String?
  }

  /// Tests of the project, and tests that ran but are gone, by name.
  private var tests: [(file: String, name: String)] {
    guard let root = project.rootURL else { return [] }
    var seen: [String: String] = [:]
    for test in project.items.flatMap(\.tests) {
      seen[RelativePath.from(root, to: test.url)] = test.name
    }
    for run in project.runs where seen[run.report.file] == nil {
      seen[run.report.file] = run.report.test
    }
    return seen.map { ($0.key, $0.value) }.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
  }

  private func count() async {
    let runs = project.runs
    if let test = project.statisticsTest {
      let own = runs.filter { $0.report.file == test }
      let offset = Int32(TimeZone.current.secondsFromGMT())
      let statistics = await Task.detached(priority: .userInitiated) {
        testStatistics(runs: own.map(\.report), utcOffsetS: offset)
      }.value
      shown = .test(test, statistics, runs: own)
    } else {
      let now = Int64(Date.now.timeIntervalSince1970 * 1000)
      let statistics = await Task.detached(priority: .userInitiated) {
        projectStatistics(runs: runs.map(\.report), nowMs: now)
      }.value
      shown = .project(statistics)
    }
  }

  private func open(_ run: SavedRun) {
    project.showRun(run.url)
    openWindow(id: SceneID.main)
  }
}

// MARK: - A test

private struct TestStatisticsView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.openWindow) private var openWindow
  let statistics: TestStatistics
  /// The runs the statistics were counted from, in the same order.
  let runs: [SavedRun]
  let open: (SavedRun) -> Void

  var body: some View {
    if statistics.runs == 0 {
      ContentUnavailableView {
        Label("No runs yet", systemImage: "chart.bar")
      } description: {
        Text("Statistics appear once the test has run.")
      }
    } else {
      ScrollView {
        VStack(alignment: .leading, spacing: 24) {
          summary
          recent
          trend
          if !statistics.failingBlocks.isEmpty { failingBlocks }
        }
        .padding(24)
        .frame(maxWidth: 900, alignment: .leading)
        .frame(maxWidth: .infinity)
      }
    }
  }

  private var summary: some View {
    Grid(horizontalSpacing: 12, verticalSpacing: 12) {
      GridRow {
        Tile(title: Text("Passed")) {
          Text(statistics.passRate ?? 0, format: .percent.precision(.fractionLength(0...1)))
        } detail: {
          Text(counts)
        }
        Tile(title: Text("Duration")) {
          Text(verbatim: statistics.duration.map { RunHistory.duration($0.averageMs) } ?? "—")
        } detail: {
          if let duration = statistics.duration {
            Text("on average · 95% of runs within \(RunHistory.duration(duration.p95Ms))")
          }
        }
        Tile(title: Text("Stability")) {
          Text(Self.rating(statistics.stability.rating))
            .foregroundStyle(Self.color(statistics.stability.rating))
        } detail: {
          Text(Self.stabilityDetail(statistics.stability))
        }
      }
    }
  }

  private var counts: String {
    var parts = [String(localized: "\(Int(statistics.runs)) runs")]
    if statistics.failed > 0 { parts.append(String(localized: "\(Int(statistics.failed)) failed")) }
    if statistics.passedOnRetry > 0 {
      parts.append(String(localized: "\(Int(statistics.passedOnRetry)) passed on a retry"))
    }
    var text = parts.joined(separator: " · ")
    if let first = statistics.firstRun.map(RunHistory.date) {
      text += "\n" + String(localized: "Since \(first.formatted(date: .abbreviated, time: .omitted))")
    }
    return text
  }

  // MARK: Latest runs

  private var recent: some View {
    VStack(alignment: .leading, spacing: 8) {
      HStack(alignment: .firstTextBaseline) {
        Text("Latest Runs").font(.headline).accessibilityAddTraits(.isHeader)
        Spacer()
        if let since = statistics.failingSince.map(RunHistory.date) {
          Label {
            Text("Failing since \(since.formatted(date: .abbreviated, time: .shortened))")
          } icon: {
            Image(systemName: RunStatus.failed.symbol)
          }
          .font(.callout)
          .foregroundStyle(.red)
        }
      }
      HStack(spacing: 3) {
        ForEach(statistics.recent, id: \.index) { run in
          Button {
            if runs.indices.contains(Int(run.index)) { open(runs[Int(run.index)]) }
          } label: {
            RoundedRectangle(cornerRadius: 2)
              .fill(Self.color(run.mark))
              .frame(maxWidth: 12)
              .frame(height: 22)
          }
          .buttonStyle(.plain)
          .help(Text(Self.describe(run)))
          .accessibilityLabel(Text(Self.describe(run)))
          .accessibilityHint(Text("Opens the report"))
        }
        Spacer(minLength: 0)
      }
      Text("Oldest on the left. Click a run to open its report.")
        .font(.caption)
        .foregroundStyle(.secondary)
    }
  }

  // MARK: Trend

  private var trend: some View {
    VStack(alignment: .leading, spacing: 8) {
      Text(Self.trendTitle(statistics.period)).font(.headline).accessibilityAddTraits(.isHeader)
      Chart {
        ForEach(statistics.trend, id: \.startMs) { point in
          let date = Date(timeIntervalSince1970: Double(point.startMs) / 1000)
          BarMark(x: .value("Period", date, unit: unit), y: .value("Runs", point.passed))
            .foregroundStyle(by: .value("Result", String(localized: "Passed")))
          BarMark(x: .value("Period", date, unit: unit), y: .value("Runs", point.passedOnRetry))
            .foregroundStyle(by: .value("Result", String(localized: "Passed on a retry")))
          BarMark(x: .value("Period", date, unit: unit), y: .value("Runs", point.failed))
            .foregroundStyle(by: .value("Result", String(localized: "Failed")))
        }
      }
      .chartForegroundStyleScale([
        String(localized: "Passed"): Color.green,
        String(localized: "Passed on a retry"): Color.orange,
        String(localized: "Failed"): Color.red,
      ])
      .frame(height: 180)
      Chart {
        ForEach(statistics.trend, id: \.startMs) { point in
          let date = Date(timeIntervalSince1970: Double(point.startMs) / 1000)
          LineMark(x: .value("Period", date, unit: unit), y: .value("Seconds", Double(point.averageMs) / 1000))
          PointMark(x: .value("Period", date, unit: unit), y: .value("Seconds", Double(point.averageMs) / 1000))
        }
      }
      .chartYAxisLabel { Text("Average duration, s") }
      .frame(height: 140)
    }
  }

  private var unit: Calendar.Component {
    switch statistics.period {
    case .day: .day
    case .week: .weekOfYear
    case .month: .month
    }
  }

  // MARK: Failing blocks

  private var failingBlocks: some View {
    VStack(alignment: .leading, spacing: 8) {
      Text("Where It Fails").font(.headline).accessibilityAddTraits(.isHeader)
      VStack(spacing: 0) {
        ForEach(Array(statistics.failingBlocks.enumerated()), id: \.offset) { index, block in
          if index > 0 { Divider() }
          HStack(alignment: .top, spacing: 12) {
            Image(systemName: BlockInfo.symbol(block.blockType))
              .foregroundStyle(.secondary)
              .frame(width: 20)
              .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 4) {
              Text(verbatim: place(block)).font(.body.weight(.medium))
              ForEach(Array(block.reasons.enumerated()), id: \.offset) { _, reason in
                Text("\(Int(reason.count))× \(RunText.failure(reason.failure))")
                  .font(.callout)
                  .foregroundStyle(.secondary)
                  .fixedSize(horizontal: false, vertical: true)
              }
              Text("Last failed \(RunHistory.date(block.lastFailed).formatted(date: .abbreviated, time: .shortened))")
                .font(.caption)
                .foregroundStyle(.tertiary)
            }
            Spacer()
            Text("\(Int(block.failures)) failures")
              .monospacedDigit()
              .foregroundStyle(.secondary)
            Button("Show Block") {
              project.showBlock(block.block, in: block.test)
              openWindow(id: SceneID.main)
            }
            .disabled(!project.hasTest(block.test))
          }
          .padding(12)
        }
      }
      .background(.background, in: .rect(cornerRadius: 12))
      .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(.separator))
    }
  }

  /// The block's type and id, and the test it's in when another test runs it.
  private func place(_ block: BlockFailures) -> String {
    var text = "\(BlockInfo.title(block.blockType)) “\(block.block)”"
    if block.test != project.statisticsTest {
      text += " · " + String(localized: "in \(ProjectScanner.testName((block.test as NSString).lastPathComponent))")
    }
    return text
  }

  // MARK: Words

  static func color(_ mark: HealthMark) -> Color {
    switch mark {
    case .passed: .green
    case .flaky: .orange
    case .failed: .red
    }
  }

  static func describe(_ run: RecentRun) -> String {
    let result =
      switch run.mark {
      case .passed: String(localized: "Passed")
      case .flaky: String(localized: "Passed on a retry")
      case .failed: String(localized: "Failed")
      }
    let when = RunHistory.date(run.startedAt).formatted(date: .abbreviated, time: .shortened)
    return "\(result) · \(when) · \(RunHistory.duration(run.durationMs))"
  }

  static func rating(_ rating: StabilityRating) -> String {
    switch rating {
    case .unknown: String(localized: "Not known yet")
    case .stable: String(localized: "Stable")
    case .sometimesFlaky: String(localized: "Sometimes flaky")
    case .flaky: String(localized: "Flaky")
    }
  }

  static func color(_ rating: StabilityRating) -> Color {
    switch rating {
    case .unknown: .secondary
    case .stable: .green
    case .sometimesFlaky: .orange
    case .flaky: .red
    }
  }

  static func stabilityDetail(_ stability: Stability) -> String {
    guard let flakiness = stability.flakiness else {
      return String(localized: "It takes two runs of the same version of the test, in the same environment.")
    }
    var text = String(
      localized: "Flakiness \(flakiness.formatted(.percent.precision(.fractionLength(0...1)))): the result changed \(Int(stability.changes)) times in \(Int(stability.comparisons)) runs without the test changing")
    if stability.passedOnRetry > 0 {
      text += String(localized: ", and \(Int(stability.passedOnRetry)) runs passed only on a retry")
    }
    return text + "."
  }

  static func trendTitle(_ period: StatisticsPeriod) -> String {
    switch period {
    case .day: String(localized: "Runs by Day")
    case .week: String(localized: "Runs by Week")
    case .month: String(localized: "Runs by Month")
    }
  }
}

// MARK: - The project

private struct ProjectStatisticsView: View {
  let statistics: ProjectStatistics
  let show: (String) -> Void

  var body: some View {
    if statistics.runs == 0 {
      ContentUnavailableView {
        Label("No runs in the last 30 days", systemImage: "chart.bar")
      } description: {
        Text("Statistics appear once tests have run.")
      }
    } else {
      ScrollView {
        VStack(alignment: .leading, spacing: 24) {
          Grid(horizontalSpacing: 12) {
            GridRow {
              Tile(title: Text("Passed")) {
                Text(statistics.passRate ?? 0, format: .percent.precision(.fractionLength(0...1)))
              } detail: {
                Text("of \(Int(statistics.runs)) runs in the last 30 days")
              }
              Tile(title: Text("Tests")) {
                Text(Int(statistics.tests), format: .number)
              } detail: {
                Text("ran in the last 30 days")
              }
            }
          }
          table(Text("Fail Most Often"), statistics.mostFailing, empty: Text("No test failed in the last 30 days.")) {
            Text("\(Int($0.failed)) of \(Int($0.runs)) failed")
          }
          table(Text("Slowest"), statistics.slowest, empty: nil) { test in
            Text("\(RunHistory.duration(test.duration.medianMs)) typical · \(RunHistory.duration(test.duration.p95Ms)) at most for 95%")
          }
        }
        .padding(24)
        .frame(maxWidth: 900, alignment: .leading)
        .frame(maxWidth: .infinity)
      }
    }
  }

  private func table(
    _ title: Text, _ tests: [TestSummary], empty: Text?, @ViewBuilder value: @escaping (TestSummary) -> Text
  ) -> some View {
    VStack(alignment: .leading, spacing: 8) {
      title.font(.headline).accessibilityAddTraits(.isHeader)
      if tests.isEmpty, let empty {
        empty.foregroundStyle(.secondary)
      } else {
        VStack(spacing: 0) {
          ForEach(Array(tests.enumerated()), id: \.offset) { index, test in
            if index > 0 { Divider() }
            Button { show(test.file) } label: {
              HStack(spacing: 10) {
                Circle().fill(TestStatisticsView.color(test.last)).frame(width: 8, height: 8)
                  .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                  Text(verbatim: test.name)
                  Text(verbatim: test.file).font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                value(test).monospacedDigit().foregroundStyle(.secondary)
                Image(systemName: "chevron.right").foregroundStyle(.tertiary).accessibilityHidden(true)
              }
              .padding(12)
              .contentShape(.rect)
            }
            .buttonStyle(.plain)
            .accessibilityHint(Text("Shows the test’s statistics"))
          }
        }
        .background(.background, in: .rect(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(.separator))
      }
    }
  }
}

/// One figure with what it means.
private struct Tile<Value: View, Detail: View>: View {
  let title: Text
  @ViewBuilder let value: Value
  @ViewBuilder let detail: Detail

  var body: some View {
    VStack(alignment: .leading, spacing: 4) {
      title.font(.callout).foregroundStyle(.secondary)
      value.font(.title.weight(.semibold)).monospacedDigit()
      detail.font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
      Spacer(minLength: 0)
    }
    .frame(maxWidth: .infinity, alignment: .leading)
    .padding(14)
    .background(.background, in: .rect(cornerRadius: 12))
    .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(.separator))
    .accessibilityElement(children: .combine)
  }
}
