import Foundation
import JEVisionCore
import SwiftUI

/// How a saved run ended, as the sidebar and the report show it: by shape and colour.
enum RunStatus: Equatable {
  case passed, failed, flaky, cancelled

  init(_ report: RunReport) {
    switch report.verdict {
    case .passed: self = report.flaky ? .flaky : .passed
    case .failed: self = .failed
    case .cancelled: self = .cancelled
    }
  }

  /// A small shape for the test's row in the sidebar.
  var dot: String {
    switch self {
    case .passed: "circle.fill"
    case .failed: "square.fill"
    case .flaky: "triangle.fill"
    case .cancelled: "circle.dotted"
    }
  }

  var symbol: String {
    switch self {
    case .passed: "checkmark.circle.fill"
    case .failed: "xmark.square.fill"
    case .flaky: "exclamationmark.triangle.fill"
    case .cancelled: "stop.circle.fill"
    }
  }

  var color: Color {
    switch self {
    case .passed: .green
    case .failed: .red
    case .flaky: .orange
    case .cancelled: .secondary
    }
  }

  var title: String {
    switch self {
    case .passed: String(localized: "Passed")
    case .failed: String(localized: "Failed")
    case .flaky: String(localized: "Flaky")
    case .cancelled: String(localized: "Stopped")
    }
  }
}

extension SavedRun: @retroactive Identifiable {
  public var id: String { dir }
  var url: URL { URL(filePath: dir, directoryHint: .isDirectory) }
  var status: RunStatus { RunStatus(report) }
  var startedAt: Date { RunHistory.date(report.startedAt) }
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

  /// Runs of tests whose name contains `query`, newest first, by day.
  static func days(_ runs: [SavedRun], matching query: String = "", calendar: Calendar = .current) -> [Day] {
    let query = query.trimmingCharacters(in: .whitespaces)
    let shown = runs
      .filter { query.isEmpty || $0.report.test.localizedStandardContains(query) }
      .sorted { ($0.startedAt, $0.dir) > ($1.startedAt, $1.dir) }
    var days: [Day] = []
    for run in shown {
      let day = calendar.startOfDay(for: run.startedAt)
      if days.last?.day == day {
        days[days.count - 1].runs.append(run)
      } else {
        days.append(Day(day: day, runs: [run]))
      }
    }
    return days
  }

  /// How the latest run of each test ended, by test file relative to the project.
  static func latest(_ runs: [SavedRun]) -> [String: RunStatus] {
    var latest: [String: (date: Date, status: RunStatus)] = [:]
    for run in runs where (latest[run.report.file]?.date ?? .distantPast) <= run.startedAt {
      latest[run.report.file] = (run.startedAt, run.status)
    }
    return latest.mapValues(\.status)
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
    let tries = report.attempts.count
    guard tries > 1 else { return nil }
    switch report.verdict {
    case .passed:
      return String(localized: "Passed on retry \(tries - 1) after failing — the test is flaky.")
    case .failed where report.flaky:
      return String(localized: "Failed \(tries) of \(tries) times, each time differently — the test may be flaky.")
    case .failed:
      return String(localized: "Failed \(tries) of \(tries) times the same way — a real failure, not a flaky test.")
    case .cancelled:
      return nil
    }
  }

  /// The step that failed the last try: the innermost one, inside loops and other tests.
  static func failedStep(_ report: RunReport) -> Step? {
    report.attempts.last?.steps.last { $0.status == .failed && $0.failure != nil }
  }
}
