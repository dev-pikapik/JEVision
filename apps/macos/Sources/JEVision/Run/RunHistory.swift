import Foundation
import JEVisionCore
import SwiftUI

/// How a saved run ended, as the sidebar and the report show it: by shape and colour.
enum RunStatus: Equatable, CaseIterable {
  /// `quarantined`: failed while the test was in quarantine, so it didn't fail Run All.
  /// `unchecked`: finished without any block looking at the screen, so it proves nothing.
  /// `blocked`: didn't run because a test it depends on didn't pass.
  case passed, failed, flaky, quarantined, cancelled, unchecked, blocked, unknown

  init(_ report: RunReport) {
    switch report.verdict {
    case .passed where report.nothingChecked: self = .unchecked
    case .passed: self = report.flaky ? .flaky : .passed
    case .failed: self = report.quarantined ? .quarantined : .failed
    case .cancelled: self = .cancelled
    case .blocked: self = .blocked
    case .unknown: self = .unknown
    }
  }

  /// A small shape for the test's row in the sidebar.
  var dot: String {
    switch self {
    case .passed: "circle.fill"
    case .failed: "square.fill"
    case .flaky: "triangle.fill"
    case .quarantined: "bandage.fill"
    case .cancelled: "circle.dotted"
    case .unchecked: "circle.dashed"
    case .blocked: "minus.circle.fill"
    case .unknown: "questionmark.circle"
    }
  }

  var symbol: String {
    switch self {
    case .passed: "checkmark.circle.fill"
    case .failed: "xmark.square.fill"
    case .flaky: "exclamationmark.triangle.fill"
    case .quarantined: "bandage.fill"
    case .cancelled: "stop.circle.fill"
    case .unchecked: "eye.slash.circle.fill"
    case .blocked: "minus.circle.fill"
    case .unknown: "questionmark.circle.fill"
    }
  }

  var color: Color {
    switch self {
    case .passed: .green
    case .failed: .red
    case .flaky: .orange
    case .quarantined: .indigo
    case .cancelled, .unchecked, .unknown: .secondary
    case .blocked: .yellow
    }
  }

  var title: String {
    switch self {
    case .passed: String(localized: "Passed")
    case .failed: String(localized: "Failed")
    case .flaky: String(localized: "Flaky")
    case .quarantined: String(localized: "Failed in Quarantine")
    case .cancelled: String(localized: "Stopped")
    case .unchecked: String(localized: "Nothing Checked")
    case .blocked: String(localized: "Blocked")
    case .unknown: String(localized: "Unknown Status")
    }
  }
}

extension HealthMark {
  /// The status the mark stands for, to draw it with the shape and color of that status.
  var status: RunStatus {
    switch self {
    case .passed: .passed
    case .flaky: .flaky
    case .failed: .failed
    }
  }
}

/// Where a run was started, to filter the history by.
enum RunSource: CaseIterable, Identifiable {
  case thisMac, schedule, phone, ci

  var id: Self { self }

  var title: String {
    switch self {
    case .thisMac: String(localized: "This Mac")
    case .schedule: String(localized: "Schedule")
    case .phone: String(localized: "iPhone")
    case .ci: String(localized: "CI")
    }
  }

  var symbol: String {
    switch self {
    case .thisMac: "desktopcomputer"
    case .schedule: "calendar.badge.clock"
    case .phone: "iphone"
    case .ci: "gearshape.2"
    }
  }

  init(_ report: RunReport) {
    if report.ci != nil {
      self = .ci
    } else if report.requestedBy != nil {
      self = .phone
    } else if report.schedule != nil {
      self = .schedule
    } else {
      self = .thisMac
    }
  }
}

extension SavedRun: @retroactive Identifiable {
  public var id: String { dir }
  var url: URL { URL(filePath: dir, directoryHint: .isDirectory) }
  var status: RunStatus { RunStatus(report) }
  var startedAt: Date { RunHistory.date(report.startedAt) }
  var source: RunSource { RunSource(report) }
  /// Made on this Mac or started from it. A CI run is of another checkout and says nothing about
  /// the test here, so what a test shows as its last run leaves it out.
  var isHere: Bool { report.ci == nil }
}

/// Runs of the project in the order and groups the sidebar shows them.
enum RunHistory {
  struct Day: Identifiable, Equatable {
    /// Start of the day, in the user's calendar.
    let day: Date
    var runs: [SavedRun]
    var id: Date { day }
  }

  /// The time a report gives in RFC 3339; the distant past if it can't be read.
  static func date(_ text: String) -> Date {
    (try? Date(text, strategy: Date.ISO8601FormatStyle(includingFractionalSeconds: true)))
      ?? (try? Date(text, strategy: .iso8601))
      ?? .distantPast
  }

  /// Runs of tests whose name contains `query`, started from `source` (any when `nil`), newest
  /// first, by day.
  static func days(
    _ runs: [SavedRun], matching query: String = "", from source: RunSource? = nil, calendar: Calendar = .current
  ) -> [Day] {
    let query = query.trimmingCharacters(in: .whitespaces)
    // Each time is read once: reading them inside the sort took most of a keystroke.
    let shown = runs
      .filter { source == nil || $0.source == source }
      .filter { query.isEmpty || $0.report.test.localizedStandardContains(query) }
      .map { (run: $0, date: $0.startedAt) }
      .sorted { ($0.date, $0.run.dir) > ($1.date, $1.run.dir) }
    var days: [Day] = []
    for (run, date) in shown {
      let day = calendar.startOfDay(for: date)
      if days.last?.day == day {
        days[days.count - 1].runs.append(run)
      } else {
        days.append(Day(day: day, runs: [run]))
      }
    }
    return days
  }

  /// The open run once the list shows only runs started from `source`: it stays only while the
  /// list still has it, so the report beside the list is always of a run in it.
  static func selection(_ id: SavedRun.ID?, in runs: [SavedRun], from source: RunSource?) -> SavedRun.ID? {
    runs.contains { $0.id == id && (source == nil || $0.source == source) } ? id : nil
  }

  /// Who started the run, for its row: the phone's name, the schedule, the CI job or this Mac.
  static func origin(_ report: RunReport) -> String {
    if let ci = report.ci {
      if let number = ci.pullRequest { return String(localized: "CI · pull request #\(number)") }
      if let branch = ci.branch { return String(localized: "CI · branch \(branch)") }
      return String(localized: "CI")
    }
    if let phone = report.requestedBy { return phone }
    if let schedule = report.schedule { return String(localized: "Schedule · \(schedule)") }
    return String(localized: "This Mac")
  }

  /// The page of the CI run. A report from CI is not trusted: only a web address is opened.
  static func link(_ ci: CiOrigin) -> URL? {
    guard let text = ci.runUrl, let url = URL(string: text), url.scheme == "https", url.host() != nil else { return nil }
    return url
  }

  /// "Today", "Yesterday" or the date, for a section header.
  static func title(of day: Date, now: Date = .now, calendar: Calendar = .current) -> String {
    if calendar.isDate(day, inSameDayAs: now) { return String(localized: "Today") }
    if let yesterday = calendar.date(byAdding: .day, value: -1, to: now), calendar.isDate(day, inSameDayAs: yesterday) {
      return String(localized: "Yesterday")
    }
    let sameYear = calendar.isDate(day, equalTo: now, toGranularity: .year)
    let style = Date.FormatStyle(calendar: calendar).weekday(.wide).month(.wide).day()
    return day.formatted(sameYear ? style : style.year())
  }

  /// A run's or step's length: tenths of a second under a minute, whole seconds above.
  static func duration(_ ms: UInt64) -> String {
    let time = Duration.milliseconds(Int64(clamping: ms))
    if ms < 60_000 {
      return time.formatted(.units(allowed: [.seconds], width: .abbreviated, fractionalPart: .show(length: 1)))
    }
    return time.formatted(.units(allowed: [.hours, .minutes, .seconds], width: .abbreviated))
  }

  /// Why a saved run can't be read or its report saved.
  static func reason(_ error: any Error) -> String {
    if case ReportError.Failed(let reason) = error { return reason }
    return error.localizedDescription
  }

  /// What the retries say about a failure: a real one, or a flaky test. `nil` for a single try.
  static func retryVerdict(_ report: RunReport) -> String? {
    JEVisionCore.retryVerdict(report: report).map { describeRetryVerdict(verdict: $0) }
  }

  /// Why a test counts as flaky, from its runs of the last 30 days.
  static func flakyEvidence(_ evidence: FlakyEvidence) -> String {
    let since = date(evidence.since).formatted(date: .abbreviated, time: .omitted)
    // Only what happened: a test that passed on retries has no run that failed every try.
    var counts: [String] = []
    if evidence.passed > 0 { counts.append(String(localized: "Passed: \(evidence.passed)")) }
    if evidence.passedOnRetry > 0 { counts.append(String(localized: "Passed on retry: \(evidence.passedOnRetry)")) }
    if evidence.failed > 0 { counts.append(String(localized: "Failed: \(evidence.failed)")) }
    let what =
      evidence.environment.map {
        String(localized: "Flaky: since \(since), the unchanged test both passed and failed in \($0).")
      } ?? String(localized: "Flaky: since \(since), the unchanged test both passed and failed.")
    let row = evidence.row.map { " " + String(localized: "Data row: \(RunChoices.rowTitle(number: Int($0.number), label: $0.label)).") } ?? ""
    return what + row + " " + counts.joined(separator: " · ")
  }

  /// The step that failed the deciding try: the innermost one, inside loops and other tests.
  static func failedStep(_ report: RunReport) -> Step? {
    decidingAttempt(report: report)?.steps.last { $0.status == .failed && $0.failure != nil }
  }
}
