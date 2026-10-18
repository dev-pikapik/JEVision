import Foundation

/// Times of schedules: minutes since 1970-01-01 00:00 on the local wall clock, as the core
/// counts them, so a run at 09:00 stays at nine o'clock across daylight saving time.
public enum LocalMinute {
  public static func of(_ date: Date, in zone: TimeZone = .current) -> Int64 {
    let seconds = date.timeIntervalSince1970 + Double(zone.secondsFromGMT(for: date))
    return Int64((seconds / 60).rounded(.down))
  }

  /// The moment the local clock shows `minute`; in the hour skipped when clocks go forward,
  /// the moment just after it.
  public static func date(_ minute: Int64, in zone: TimeZone = .current) -> Date {
    let wall = Date(timeIntervalSince1970: Double(minute) * 60)
    let guess = wall.addingTimeInterval(-Double(zone.secondsFromGMT(for: wall)))
    return wall.addingTimeInterval(-Double(zone.secondsFromGMT(for: guess)))
  }
}
