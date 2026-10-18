import Foundation
import JEVisionCore

/// What the saved runs say about the open test: the toolbar's last-run status, the marks on the
/// blocks and the reason a selected block failed.
struct LastRun {
  /// The newest run of the test itself.
  let own: SavedRun?
  /// The run the blocks show.
  let shown: SavedRun?
  /// How each block did in `shown`, by block id.
  let results: [String: BlockResult]
  /// Ways out of the `if`s of the test `shown` took.
  let taken: Set<Slot>

  /// `file` is the test relative to the project; `pinned` the run a report sent the user here from.
  init(file: String, flow: Flow, runs: [SavedRun], pinned: SavedRun.ID?) {
    own = Self.own(file, in: runs)
    shown = runs.first { $0.id == pinned } ?? Self.ran(file, in: runs)
    results = shown.map {
      Dictionary(uniqueKeysWithValues: blockResults(flow: flow, file: file, report: $0.report).map { ($0.block, $0) })
    } ?? [:]
    let steps = shown.flatMap { decidingAttempt(report: $0.report) }?.steps ?? []
    taken = Set(steps.compactMap { step in step.test == file ? step.branch.map { Slot(from: step.block, edge: $0) } : nil })
  }

  /// The newest run of `file`, from the runs newest first.
  static func own(_ file: String, in runs: [SavedRun]) -> SavedRun? {
    runs.first { $0.report.file == file }
  }

  /// The newest run in which blocks of `file` ran: its own, or one of a test that ran it.
  static func ran(_ file: String, in runs: [SavedRun]) -> SavedRun? {
    runs.first { run in
      run.report.file == file || decidingAttempt(report: run.report)?.steps.contains { $0.test == file } == true
    }
  }

  /// The selected block's failure in `shown`, for the inspector.
  func failure(of block: String) -> BlockFailure? {
    guard let shown, let result = results[block], result.outcome == .failed else { return nil }
    return BlockFailure(run: shown, failure: result.failure)
  }
}

struct BlockFailure {
  let run: SavedRun
  let failure: Failure?
}
