import JEVisionCore

/// What a run is doing, built from its events: what the HUD shows and the canvas marks.
struct RunState: Equatable {
  enum Phase: Equatable {
    case running
    case paused(PauseReason)
    case finished(Verdict)
  }

  enum Mark: Equatable {
    case passed, failed
  }

  /// The test file, relative to the project, as the run reports it.
  private(set) var file = ""
  private(set) var name = ""
  private(set) var phase = Phase.running
  private(set) var attempt: UInt32 = 1
  private(set) var maxAttempts: UInt32 = 1
  /// The block of this test that runs now or is about to; `nil` while a test it runs is busy.
  private(set) var current: String?
  /// Type of the block that runs now, wherever it is.
  private(set) var currentType: String?
  /// How each block of this test ended in the current attempt.
  private(set) var marks: [String: Mark] = [:]
  private(set) var flaky = false
  /// The last attempt's reason to fail.
  private(set) var failure: Failure?

  /// Steps started and not yet finished, innermost last: a loop or a run of another test.
  private var open: [OpenStep] = []

  private struct OpenStep: Equatable {
    let test: String, block: String, type: String
  }

  var isPaused: Bool {
    if case .paused = phase { true } else { false }
  }

  var isFinished: Bool {
    if case .finished = phase { true } else { false }
  }

  /// Blocks of this test done so far in this attempt, counting each once.
  var done: Int { marks.count }

  mutating func apply(_ event: RunEvent) {
    switch event {
    case .runStarted(let test, let file, _, let maxAttempts):
      self.file = file
      name = test
      self.maxAttempts = max(1, maxAttempts)
    case .attemptStarted(let attempt):
      self.attempt = attempt
      marks = [:]
      open = []
      current = nil
      currentType = nil
      phase = .running
    case .stepStarted(_, _, let test, let block, let blockType, _):
      open.append(OpenStep(test: test, block: block, type: blockType))
      current = test == file ? block : nil
      currentType = blockType
      phase = .running
    case .stepFinished(_, let step):
      if let index = open.lastIndex(where: { $0.test == step.test && $0.block == step.block }) {
        open.removeSubrange(index...)
      }
      if step.test == file {
        switch step.status {
        case .passed, .baselineCreated: marks[step.block] = .passed
        case .failed: marks[step.block] = .failed
        case .cancelled: break
        }
      }
      current = open.last.flatMap { $0.test == file ? $0.block : nil }
      currentType = open.last?.type
    case .paused(let block, let reason):
      // A pause comes before its block starts, so the block belongs to the innermost open step's
      // test; inside a run of another test, which one isn't known here.
      if isInThisTest {
        if open.last?.block != block { currentType = nil }
        current = block
      }
      phase = .paused(reason)
    case .resumed:
      phase = .running
    case .attemptFinished(_, _, let failure):
      self.failure = failure
    case .runFinished(let verdict, let flaky, _):
      self.flaky = flaky
      phase = .finished(verdict)
    }
  }

  private var isInThisTest: Bool {
    guard let last = open.last else { return true }
    return last.test == file && last.type != "runTest"
  }
}
