import Foundation
import JEVisionCore

/// Schedules and their runs in words, such as “Every weekday at 09:00 · staging”.
enum ScheduleText {
  static let weekdays: [UInt8] = [1, 2, 3, 4, 5]
  static let weekend: [UInt8] = [0, 6]

  /// When the schedule runs and, when it names one, the environment.
  static func summary(_ schedule: ProjectSchedule) -> String {
    [when(schedule), schedule.environment].compactMap(\.self).joined(separator: " · ")
  }

  static func when(_ schedule: ProjectSchedule) -> String {
    let time = self.time(hour: schedule.hour, minute: schedule.minute)
    switch schedule.repeats {
    case .daily:
      return String(localized: "Every day at \(time)")
    case .days(let days):
      let set = Set(days)
      if set == Set(weekdays) { return String(localized: "Every weekday at \(time)") }
      if set == Set(weekend) { return String(localized: "Every weekend at \(time)") }
      if set.count == 7 { return String(localized: "Every day at \(time)") }
      return String(localized: "\(dayList(Array(days))) at \(time)")
    case .everyHours(let hours):
      if hours == 1 { return String(localized: "Every hour from \(time)") }
      return String(localized: "Every \(Int(hours)) hours from \(time)")
    }
  }

  /// Short names of `days`, in the order of the week where the user lives.
  static func dayList(_ days: [UInt8]) -> String {
    let symbols = Calendar.current.shortWeekdaySymbols
    return orderedDays.filter(days.contains).map { symbols[Int($0)] }.formatted(.list(type: .and, width: .short))
  }

  /// Days of the week from the first one where the user lives, 0 for Sunday.
  static var orderedDays: [UInt8] {
    let first = UInt8(Calendar.current.firstWeekday - 1)
    return (0..<7).map { (first + $0) % 7 }
  }

  static func time(hour: UInt8, minute: UInt8) -> String {
    let date = Calendar.current.date(from: DateComponents(hour: Int(hour), minute: Int(minute))) ?? .now
    return date.formatted(date: .omitted, time: .shortened)
  }

  static func tests(_ tests: ScheduledTests) -> String {
    switch tests {
    case .all: String(localized: "All tests")
    case .folder(let path): String(localized: "Tests in “\(path)”")
    case .files(let paths):
      paths.count == 1
        ? ((paths[0] as NSString).lastPathComponent.replacing(".flow.json", with: ""))
        : String(localized: "\(paths.count) tests")
    }
  }

  static func outcome(_ run: ScheduledRun) -> String {
    switch run.outcome {
    case .finished(let passed, let failed):
      if failed == 0 { return String(localized: "Passed: \(Int(passed)) of \(Int(passed))") }
      return String(localized: "Failed: \(Int(failed)) of \(Int(passed + failed))")
    case .skipped(.screenLocked): return String(localized: "Skipped: the screen was locked")
    case .skipped(.noUser): return String(localized: "Skipped: nobody was logged in")
    case .skipped(.notAllowed): return String(localized: "Skipped: the jevision tool isn’t allowed to control this Mac")
    case .error(let message): return String(localized: "Couldn’t start: \(message)")
    }
  }

  static func notificationTitle(_ run: ScheduledRun) -> String {
    switch run.outcome {
    case .finished: String(localized: "“\(run.schedule)” failed")
    case .skipped: String(localized: "“\(run.schedule)” didn’t run")
    case .error: String(localized: "“\(run.schedule)” couldn’t start")
    }
  }

  /// `today at 09:00`, `tomorrow at 09:00`, or the date.
  static func moment(_ date: Date) -> String {
    let time = date.formatted(date: .omitted, time: .shortened)
    let calendar = Calendar.current
    if calendar.isDateInToday(date) { return String(localized: "today at \(time)") }
    if calendar.isDateInTomorrow(date) { return String(localized: "tomorrow at \(time)") }
    if calendar.isDateInYesterday(date) { return String(localized: "yesterday at \(time)") }
    return date.formatted(.dateTime.weekday(.abbreviated).day().month(.abbreviated).hour().minute())
  }
}
