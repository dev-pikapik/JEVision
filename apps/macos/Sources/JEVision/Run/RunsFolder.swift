import Foundation
import JEVisionCore

/// The date and size of a file, to tell whether it changed since it was last read.
struct FileStamp: Equatable, Sendable {
  let seconds: Int
  let nanoseconds: Int
  let size: Int64

  /// The stamp of the file at `path`; `nil` when there is no such file.
  init?(path: String) {
    var info = stat()
    guard stat(path, &info) == 0, info.st_mode & S_IFMT == S_IFREG else { return nil }
    seconds = info.st_mtimespec.tv_sec
    nanoseconds = info.st_mtimespec.tv_nsec
    size = info.st_size
  }
}

/// The runs saved in a project, read again only where they changed: a project keeps thousands
/// of them, and reading every report again after each run took seconds. One read at a time.
final class RunsFolder: @unchecked Sendable {
  /// What a read found.
  struct Found: Sendable {
    /// Every run, newest first, as `listRuns` gives them.
    let runs: [SavedRun]
    let index: RunIndex
    /// The runs new or changed since the read before.
    let changed: Set<SavedRun.ID>
  }

  private struct Known {
    let stamp: FileStamp
    /// `nil` for a report that can't be read: it is tried again once it changes.
    let run: SavedRun?
  }

  /// `<project>/.jevision/runs/`, as the core spells the folders of the runs.
  private let folder: String
  private let lock = NSLock()
  private var known: [String: Known] = [:]
  private var reads = 0

  init(project: URL) {
    folder = project.path + "/.jevision/runs/"
  }

  /// Reports read, over all reads.
  var reportCount: Int { lock.withLock { reads } }

  /// The runs as they are now; `nil` when no report came, went or changed since the read before.
  func read() -> Found? {
    lock.lock()
    defer { lock.unlock() }
    let names = (try? FileManager.default.contentsOfDirectory(atPath: folder)) ?? []
    var found = [String: Known](minimumCapacity: names.count)
    var stale: [(name: String, stamp: FileStamp)] = []
    for name in names {
      // A run still going has no report yet.
      guard let stamp = FileStamp(path: folder + name + "/report.json") else { continue }
      if let old = known[name], old.stamp == stamp {
        found[name] = old
      } else {
        stale.append((name, stamp))
      }
    }
    guard !stale.isEmpty || found.count != known.count else { return nil }
    // One after another: read side by side, the reports took three times the processor time.
    var changed = Set<SavedRun.ID>()
    for (name, stamp) in stale {
      let run = try? loadRun(dir: folder + name)
      found[name] = Known(stamp: stamp, run: run)
      if let run { changed.insert(run.id) }
    }
    reads += stale.count
    known = found
    // RFC 3339 times in UTC sort as text; the folder breaks ties, as in the core.
    let runs = found.values.compactMap(\.run).sorted { ($0.report.startedAt, $0.dir) > ($1.report.startedAt, $1.dir) }
    return Found(runs: runs, index: RunIndex(runs), changed: changed)
  }
}

/// Finds among the runs, newest first, the ones a test shows, without looking through them all.
struct RunIndex: Sendable {
  let runs: [SavedRun]
  private let byID: [SavedRun.ID: Int]
  private let own: [String: Int]
  private let ran: [String: Int]

  init() { self.init([]) }

  /// `runs` newest first.
  init(_ runs: [SavedRun]) {
    self.runs = runs
    var byID = [SavedRun.ID: Int](minimumCapacity: runs.count)
    var own: [String: Int] = [:]
    var ran: [String: Int] = [:]
    for (position, run) in runs.enumerated() {
      byID[run.id] = byID[run.id] ?? position
      guard run.isHere else { continue }
      let file = run.report.file
      if own[file] == nil { own[file] = position }
      if ran[file] == nil { ran[file] = position }
      // Blocks of other tests count when the deciding attempt ran them. The core is asked which
      // attempt that is only for a run that went into a test no newer run went into: asking for
      // every run took a second.
      let open = run.report.attempts.contains { $0.steps.contains { $0.test != file && ran[$0.test] == nil } }
      guard open else { continue }
      for step in decidingAttempt(report: run.report)?.steps ?? [] where ran[step.test] == nil { ran[step.test] = position }
    }
    self.byID = byID
    self.own = own
    self.ran = ran
  }

  /// The run saved in the folder `id`.
  func run(_ id: SavedRun.ID?) -> SavedRun? { id.flatMap { byID[$0] }.map { runs[$0] } }

  /// The newest run of `file` made here, as `LastRun.own` finds it.
  func own(_ file: String) -> SavedRun? { own[file].map { runs[$0] } }

  /// The newest run made here in which blocks of `file` ran, as `LastRun.ran` finds it.
  func ran(_ file: String) -> SavedRun? { ran[file].map { runs[$0] } }
}
