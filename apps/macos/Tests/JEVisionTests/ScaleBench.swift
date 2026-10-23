import Foundation
import JEVisionCore

/// What the scale benchmarks share: the made-up project, written once per process, and the
/// timing that prints `BENCH\t<name>\t<wall ms>\t<cpu ms>`. Wall time moves with whatever else the
/// Mac is doing; CPU time of the process hardly does, so comparisons use it.
enum ScaleBench {
  nonisolated(unsafe) private static var written: (root: URL, summary: SyntheticSummary)?

  /// The project, written once per process into the same temporary folder, which a later
  /// process empties first. Its history ends now, so the window shows it as recent.
  static func project() throws -> (root: URL, summary: SyntheticSummary) {
    if let written { return written }
    let root = FileManager.default.temporaryDirectory.appending(path: "jevision-scale-bench-app/Scale")
    try? FileManager.default.removeItem(at: root)
    let start = ContinuousClock.now
    let summary = try writeSyntheticProject(
      dir: root.path, spec: SyntheticSpec(endMs: UInt64(Date.now.timeIntervalSince1970 * 1000)))
    report("app.generate.project", wall: milliseconds(ContinuousClock.now - start), cpu: nil)
    written = (root, summary)
    return (root, summary)
  }

  static func milliseconds(_ duration: Duration) -> Double {
    Double(duration.components.seconds) * 1000 + Double(duration.components.attoseconds) / 1e15
  }

  /// CPU time of this process (all threads) in milliseconds.
  static func cpuMilliseconds() -> Double {
    var now = timespec()
    clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &now)
    return Double(now.tv_sec) * 1000 + Double(now.tv_nsec) / 1e6
  }

  /// CPU time of the thread that asks, in milliseconds: on the main thread, what held up the window.
  static func threadCPUMilliseconds() -> Double {
    var now = timespec()
    clock_gettime(CLOCK_THREAD_CPUTIME_ID, &now)
    return Double(now.tv_sec) * 1000 + Double(now.tv_nsec) / 1e6
  }

  static func report(_ name: String, wall: Double, cpu: Double?) {
    let cpuText = cpu.map { String(format: "\t%.3f", $0) } ?? ""
    print("BENCH\t\(name)\t\(String(format: "%.3f", wall))\(cpuText)")
  }

  /// Runs `work` three times and prints the fastest wall time with the least CPU time seen.
  @discardableResult
  static func bench(_ name: String, _ work: () throws -> Void) rethrows -> (wall: Double, cpu: Double) {
    let clock = ContinuousClock()
    var best = (wall: Double.infinity, cpu: Double.infinity)
    for _ in 0..<3 {
      let cpu = cpuMilliseconds()
      let wall = milliseconds(try clock.measure(work))
      best = (min(best.wall, wall), min(best.cpu, cpuMilliseconds() - cpu))
    }
    report(name, wall: best.wall, cpu: best.cpu)
    return best
  }

  @discardableResult
  static func bench(
    _ name: String, isolation: isolated (any Actor)? = #isolation, _ work: () async throws -> Void
  ) async rethrows -> (wall: Double, cpu: Double) {
    let clock = ContinuousClock()
    var best = (wall: Double.infinity, cpu: Double.infinity)
    for _ in 0..<3 {
      let cpu = cpuMilliseconds()
      let start = clock.now
      try await work()
      let wall = milliseconds(clock.now - start)
      best = (min(best.wall, wall), min(best.cpu, cpuMilliseconds() - cpu))
    }
    report(name, wall: best.wall, cpu: best.cpu)
    return best
  }
}
