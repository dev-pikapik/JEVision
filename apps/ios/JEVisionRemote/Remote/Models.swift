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
  /// Runs tests and changes them.
  case edit

  var canRun: Bool { self != .view }
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
  /// Didn't run: a test it depends on didn't pass.
  case blocked
  /// Sent by a newer Mac; this version doesn't know what it means.
  case unknown
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
  /// What each row of the test's data table is called, when the test runs once per row; a run
  /// can be limited to one of them. Empty without such a table.
  var rows: [String] = []
  /// The card fields as the test file writes them (`critical`, `smoke`, `ready`); `nil` when
  /// the test has none and from Macs that don't send them. `CardText` names them.
  var priority: String? = nil
  var testType: String? = nil
  var status: String? = nil
  /// Who wrote the test or looks after it.
  var author: String? = nil
  /// The sticky notes on the canvas; empty without any and from Macs that don't send them.
  var notes: [TestNote] = []
  /// The version of the file this was read from, which a change names so the Mac can tell
  /// that nothing changed in between. Empty from Macs that predate editing.
  var revision = ""
  /// The blocks a change can add, in the order of the Mac's library.
  var palette: [PaletteEntry] = []

  /// The read-only drawing shifted into its scroll view, without changing wire coordinates.
  var normalizedCanvas: Self {
    let rects = blocks.compactMap(\.area) + frames.map(\.area) + notes.map(\.area)
    var bounds = rects.reduce(CGRect.null) { $0.union($1) }
    for point in edges.flatMap(\.points) {
      bounds = bounds.union(CGRect(x: point.x, y: point.y, width: 1, height: 1))
    }
    guard !bounds.isNull else { return self }
    let left = min(0, bounds.minX), top = min(0, bounds.minY)
    guard left < 0 || top < 0 else { return self }
    var copy = self
    copy.size = CGSize(width: max(size.width, bounds.maxX) - left, height: max(size.height, bounds.maxY) - top)
    for i in copy.blocks.indices { copy.blocks[i].area = copy.blocks[i].area?.offsetBy(dx: -left, dy: -top) }
    for i in copy.frames.indices { copy.frames[i].area = copy.frames[i].area.offsetBy(dx: -left, dy: -top) }
    for i in copy.notes.indices { copy.notes[i].area = copy.notes[i].area.offsetBy(dx: -left, dy: -top) }
    for i in copy.edges.indices {
      copy.edges[i].points = copy.edges[i].points.map { CGPoint(x: $0.x - left, y: $0.y - top) }
    }
    return copy
  }

  struct Block: Hashable, Identifiable, Sendable {
    var id: String
    /// The block's `type`, such as `click`.
    var kind: String
    /// The block's main parameter as written in the test; empty when the kind says it all.
    var title: String
    /// `nil` for a block the layout leaves out.
    var area: CGRect?
    /// The settings this iPhone can change, when it may edit.
    var fields: [Field] = []
    /// What the Mac finds wrong with the block.
    var problems: [Problem] = []
  }

  /// One setting of a block, with its value as the Mac shows it.
  struct Field: Hashable, Identifiable, Sendable {
    /// Where it is in the block, such as `text` or `target.description`.
    var path: String
    var kind: FieldKind
    /// `true` or `false` for a switch; empty when not set.
    var value: String
    /// The values a choice takes, as the test file writes them.
    var choices: [String] = []
    var id: String { path }
  }

  enum FieldKind: Hashable, Sendable {
    case text
    case longText
    case number
    case flag
    case choice
    /// A time such as `2s` or `500ms`.
    case duration
  }

  /// A catalog key of the core with its values; `BlockText` words it.
  struct Problem: Hashable, Sendable {
    var code: String
    var params: [String: String]
    /// The test still runs, but probably not as meant.
    var warning: Bool
  }

  struct PaletteEntry: Hashable, Sendable {
    var blockType: String
    /// The library group, such as `actions`.
    var group: String
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

/// One change to a test, made by the Mac.
enum TestEdit: Hashable, Sendable {
  /// An empty value takes an optional setting away.
  case setField(block: String, field: String, value: String)
  /// After `after`, or at the end of the test.
  case addBlock(after: String?, blockType: String)
  /// What led to the block then leads to what came after it.
  case removeBlock(String)
  /// The name the test shows; its file keeps its own.
  case rename(String)
}

/// A test as a change left it, and the block to show next: the one a change added.
struct EditedTest: Hashable, Sendable {
  var flow: TestFlow
  var select: String?
}

/// A sticky note of a test, for reading only. Its text is Markdown; the Mac has already limited
/// how much of it there is. Geometry is in canvas points, like the blocks'.
struct TestNote: Hashable, Identifiable, Sendable {
  var id: String
  var text: String
  /// Empty when unknown.
  var author: String
  var time: Date?
  var anchor: Anchor
  var area: CGRect
  /// How many replies the note has, also when only the newest are in `replies`.
  var replyCount: Int
  /// Oldest first.
  var replies: [Reply]
  /// `nil` while the discussion is open.
  var resolved: Resolved?
  var attachments: [Attachment]

  enum Anchor: Hashable, Sendable {
    case test
    case block(String)
    case folder
  }

  struct Reply: Hashable, Identifiable, Sendable {
    var id: String
    var author: String
    var time: Date?
    var text: String
  }

  struct Resolved: Hashable, Sendable {
    var by: String
    var time: Date?
  }

  enum Attachment: Hashable, Sendable {
    /// A file that stays on the Mac; only its name is here.
    case file(String)
    case link(URL)
  }
}

enum RunState: Hashable, Sendable {
  case running
  case paused
  case finished
  /// Sent by a newer Mac; this version doesn't know what it means.
  case unknown
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
  /// Set while the run waits for its turn at the screen; `nil` otherwise and from Macs that
  /// don't send it.
  var waiting: ScreenWait? = nil
}

/// A run that waits for the Mac's screen because another run has it.
struct ScreenWait: Hashable, Sendable {
  /// Who has the screen; `nil` while the lock does not say.
  var holder: ScreenHolder?
}

/// The run that has the screen: another JEVision window, the command line or the assistant.
struct ScreenHolder: Hashable, Sendable {
  /// The program, such as `JEVision` or `jevision`.
  var process: String
  /// Name of the test that runs.
  var test: String
  /// When that run took the screen.
  var since: Date
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
  /// `title` is what the block works with, its placeholders filled for this run, such as the
  /// data row's name; `nil` from Macs that don't send it.
  case blockStarted(test: String, block: String, kind: String, title: String? = nil)
  /// `branch` is the way out an `if` took, `branches` how many it had and `branchCondition`
  /// what the way taken looks for; `nil` for other blocks.
  case blockFinished(
    test: String, block: String, status: StepStatus, failure: Failure?, branch: String? = nil, branches: Int? = nil,
    branchCondition: String? = nil)
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
  /// Sent by a newer Mac; this version doesn't know what it means.
  case unknown
}

/// Why something failed: a localization key from the core and the values its text refers to.
struct Failure: Hashable, Sendable {
  var code: String
  var params: [String: String]
  /// The failure in the words of the Mac's run: what is shown for a code this version doesn't know.
  var message = ""
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
  /// The test that didn't pass, so this one, depending on it, didn't run.
  var blockedBy: String? = nil

  struct Attempt: Hashable, Sendable {
    var number: Int
    var verdict: Verdict
    var durationMs: UInt64
    var failure: Failure?
    /// Row of the test's data table the attempt ran with; `nil` for a test without one.
    var row: DataRow? = nil
    var steps: [Step]
  }

  /// A row of the test's data table, counted from 1.
  struct DataRow: Hashable, Sendable {
    var number: Int
    var label: String
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
    /// The way out an `if` took: `then`, `elseIf1`, `elseIf2`… or `else`.
    var branch: String? = nil
    /// How many ways out the `if` had; `nil` in reports from before it could have more than two.
    var branches: Int? = nil
    /// What the condition of the way taken looks for. Absent for `else`.
    var branchCondition: String? = nil
    /// What the step wants known though it didn't fail, e.g. how an app was started.
    var note: Failure? = nil
    var id: Int { seq }
  }
}
