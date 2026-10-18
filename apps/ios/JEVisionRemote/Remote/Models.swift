import CoreGraphics
import Foundation

/// A paired Mac as this iPhone saves it. Addresses change; save `MacSession.record` after
/// every sign-in.
struct HostRecord: Codable, Hashable, Identifiable, Sendable {
  /// The Mac's endpoint id, hex.
  var hostId: String
  var name: String
  /// This iPhone's id on that Mac.
  var deviceId: String
  var relayUrl: String?
  var addrs: [String]

  var id: String { hostId }
}

/// What the Mac lets this iPhone do. The Mac checks every request; this only shapes the screens.
enum Permission: Sendable {
  case view
  case run
}

struct Project: Hashable, Identifiable, Sendable {
  var name: String
  var id: String { name }
}

/// A test file or a folder, with its path relative to the project folder.
struct TestEntry: Hashable, Identifiable, Sendable {
  var path: String
  var name: String
  var isFolder: Bool
  var lastRun: LastRun?
  var id: String { path }
}

struct LastRun: Hashable, Sendable {
  var run: String
  var verdict: Verdict
  var startedAt: Date
}

enum Verdict: Hashable, Sendable {
  case passed
  case failed
  case cancelled
}

/// A test as the Mac lays it out, for drawing only. Geometry is in canvas points.
struct TestFlow: Hashable, Sendable {
  var name: String
  var description: String?
  /// Bundle id of the app under test.
  var app: String
  var start: String
  var size: CGSize
  /// In reading order.
  var blocks: [Block]
  /// Frames around loops and their bodies.
  var frames: [Frame]
  var edges: [Edge]

  struct Block: Hashable, Identifiable, Sendable {
    var id: String
    /// The block's `type`, such as `click`.
    var kind: String
    /// The block's main parameter as written in the test; empty when the kind says it all.
    var title: String
    /// `nil` for a block the layout leaves out.
    var area: CGRect?
  }

  struct Frame: Hashable, Sendable {
    var block: String
    var area: CGRect
  }

  /// A connector from `from` along its edge (`next`, `then`, `else`, `body`) to `to`, drawn
  /// through `points` with the arrow at the last one.
  struct Edge: Hashable, Sendable {
    var from: String
    var edge: String
    var to: String
    var points: [CGPoint]
  }
}

enum RunState: Hashable, Sendable {
  case running
  case paused
  case finished
}

struct RunStatus: Hashable, Sendable {
  var run: String
  /// Test file, relative to the project folder.
  var test: String
  var state: RunState
  var verdict: Verdict?
  var attempt: Int
  /// The block running now.
  var block: String?
}

struct RunSummary: Hashable, Identifiable, Sendable {
  var run: String
  /// Name of the test.
  var test: String
  var file: String
  var startedAt: Date
  var durationMs: UInt64
  var verdict: Verdict
  var flaky: Bool
  var id: String { run }
}

struct RunList: Hashable, Sendable {
  var active: [RunStatus]
  var saved: [RunSummary]
}

/// What a followed run sends: its full status first and again whenever the phone fell behind,
/// events in between.
enum RunUpdate: Hashable, Sendable {
  case status(RunStatus)
  case event(RunEvent)
}

enum RunEvent: Hashable, Sendable {
  case attemptStarted(attempt: Int)
  /// `test` is the file the block belongs to; it differs from the run's test inside `runTest`.
  case blockStarted(test: String, block: String, kind: String)
  case blockFinished(test: String, block: String, status: StepStatus, failure: Failure?)
  case paused(block: String)
  case resumed
  /// The last event of a run; its report is saved by now.
  case finished(verdict: Verdict, flaky: Bool)
}

enum StepStatus: Hashable, Sendable {
  case passed
  case failed
  /// There was no reference yet; the screenshot became the reference.
  case baselineCreated
  case cancelled
}

/// Why something failed: a localization key from the core and the values its text refers to.
struct Failure: Hashable, Sendable {
  var code: String
  var params: [String: String]
}

struct RunReport: Hashable, Sendable {
  var test: String
  var file: String
  var app: String
  var startedAt: Date
  var durationMs: UInt64
  var verdict: Verdict
  var flaky: Bool
  var baselinesCreated: [String]
  var attempts: [Attempt]
  /// The paired device that started the run; `nil` when it was started on the Mac.
  var requestedBy: String? = nil

  struct Attempt: Hashable, Sendable {
    var number: Int
    var verdict: Verdict
    var durationMs: UInt64
    var failure: Failure?
    var steps: [Step]
  }

  /// One block as it ran. Picture names are what `MacSession.picture` takes.
  struct Step: Hashable, Identifiable, Sendable {
    var seq: Int
    var test: String
    var block: String
    var kind: String
    var status: StepStatus
    var durationMs: UInt64
    var before: String?
    var after: String?
    var expected: String?
    var diff: String?
    var failure: Failure?
    var id: Int { seq }
  }
}
