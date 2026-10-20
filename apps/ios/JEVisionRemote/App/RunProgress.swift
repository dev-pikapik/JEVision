import Foundation

/// A run as followed live: what is running, what passed and failed, how it ended.
struct RunProgress: Equatable {
  struct Step: Hashable, Identifiable {
    var id: Int
    let test: String
    let block: String
    let kind: String
    /// `nil` while the block runs.
    let status: StepStatus?
    let failure: Failure?
    /// Which way an `if` went: Took “Yes”, Took “Condition 2: Inbox is empty”.
    var took: String? = nil
  }

  /// The run's test file; blocks of tests it calls belong to other files.
  private(set) var test: String?
  private(set) var state: RunState = .running
  /// Set while the run waits for its turn at the screen; its first block hasn't started yet.
  private(set) var waiting: ScreenWait?
  private(set) var attempt = 1
  /// The block of the run's own test that is running now.
  private(set) var current: String?
  private(set) var marks: [String: StepStatus] = [:]
  /// Finished steps of the current attempt, in order.
  private(set) var steps: [Step] = []
  /// The block running now, of the run's test or of a test it calls.
  private var running: Step?
  private(set) var verdict: Verdict?
  private(set) var flaky = false
  /// Why the current attempt failed: its first failure.
  private(set) var failure: Failure?
  private var kinds: [String: String] = [:]

  var isFinished: Bool { state == .finished }

  /// The steps of the current attempt in run order, oldest first: the finished ones, then the
  /// one running now. The running one keeps its id when it finishes.
  var lines: [Step] {
    guard var running else { return steps }
    running.id = steps.count
    return steps + [running]
  }

  mutating func apply(_ update: RunUpdate) {
    switch update {
    case .status(let status):
      test = status.test
      state = status.state
      if status.attempt != attempt { startAttempt(status.attempt) }
      verdict = status.verdict
      waiting = status.state == .finished ? nil : status.waiting
      current = status.state == .finished ? nil : status.block
      if let current, running?.block != current {
        running = Step(id: 0, test: status.test, block: current, kind: "", status: nil, failure: nil)
      } else if status.state == .finished {
        running = nil
      }
    case .event(let event):
      apply(event)
    }
  }

  private mutating func apply(_ event: RunEvent) {
    switch event {
    case .attemptStarted(let number):
      startAttempt(number)
    case .blockStarted(let file, let block, let kind):
      kinds[file + "\n" + block] = kind
      waiting = nil
      running = Step(id: 0, test: file, block: block, kind: kind, status: nil, failure: nil)
      if isOwn(file) { current = block }
    case .blockFinished(let file, let block, let status, let failure, let branch, let branches, let condition):
      steps.append(
        Step(
          id: steps.count, test: file, block: block, kind: kinds[file + "\n" + block] ?? "", status: status,
          failure: failure, took: branch.flatMap { RunText.branch($0, branches: branches, condition: condition) }))
      if running?.test == file, running?.block == block { running = nil }
      if isOwn(file) {
        marks[block] = status
        if current == block { current = nil }
      }
      if self.failure == nil, let failure { self.failure = failure }
    case .paused(let block):
      state = .paused
      if current == nil { current = block }
      if running == nil { running = Step(id: 0, test: test ?? "", block: block, kind: "", status: nil, failure: nil) }
    case .resumed:
      state = .running
    case .finished(let verdict, let flaky):
      state = .finished
      waiting = nil
      self.verdict = verdict
      self.flaky = flaky
      current = nil
      running = nil
    }
  }

  private func isOwn(_ file: String) -> Bool {
    test == nil || test == file
  }

  private mutating func startAttempt(_ number: Int) {
    attempt = number
    waiting = nil
    current = nil
    running = nil
    marks = [:]
    steps = []
    failure = nil
  }
}

/// The direct children of a folder in a project's flat list of tests, folders first.
enum TestTree {
  static func children(of folder: String, in entries: [TestEntry]) -> [TestEntry] {
    let prefix = folder.isEmpty ? "" : folder + "/"
    var result: [String: TestEntry] = [:]
    for entry in entries where entry.path.hasPrefix(prefix) && entry.path != folder {
      let rest = entry.path.dropFirst(prefix.count)
      if let slash = rest.firstIndex(of: "/") {
        // A test deeper down: its folder is a child even if the list doesn't name it.
        let name = String(rest[..<slash])
        let path = prefix + name
        if result[path] == nil { result[path] = TestEntry(path: path, name: name, isFolder: true) }
      } else {
        result[entry.path] = entry
      }
    }
    return result.values.sorted {
      if $0.isFolder != $1.isFolder { return $0.isFolder }
      return $0.name.localizedStandardCompare($1.name) == .orderedAscending
    }
  }
}
