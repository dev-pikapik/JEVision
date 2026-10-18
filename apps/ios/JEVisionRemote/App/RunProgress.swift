import Foundation

/// A run as followed live: what is running, what passed and failed, how it ended.
struct RunProgress: Equatable {
  struct Step: Hashable, Identifiable {
    let id: Int
    let test: String
    let block: String
    let kind: String
    let status: StepStatus
    let failure: Failure?
  }

  /// The run's test file; blocks of tests it calls belong to other files.
  private(set) var test: String?
  private(set) var state: RunState = .running
  private(set) var attempt = 1
  /// The block of the run's own test that is running now.
  private(set) var current: String?
  private(set) var marks: [String: StepStatus] = [:]
  /// Finished steps of the current attempt, in order.
  private(set) var steps: [Step] = []
  private(set) var verdict: Verdict?
  private(set) var flaky = false
  /// Why the current attempt failed: its first failure.
  private(set) var failure: Failure?
  private var kinds: [String: String] = [:]

  var isFinished: Bool { state == .finished }

  mutating func apply(_ update: RunUpdate) {
    switch update {
    case .status(let status):
      test = status.test
      state = status.state
      if status.attempt != attempt { startAttempt(status.attempt) }
      verdict = status.verdict
      current = status.state == .finished ? nil : status.block
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
      if isOwn(file) { current = block }
    case .blockFinished(let file, let block, let status, let failure):
      steps.append(
        Step(
          id: steps.count, test: file, block: block, kind: kinds[file + "\n" + block] ?? "", status: status,
          failure: failure))
      if isOwn(file) {
        marks[block] = status
        if current == block { current = nil }
      }
      if self.failure == nil, let failure { self.failure = failure }
    case .paused(let block):
      state = .paused
      if current == nil { current = block }
    case .resumed:
      state = .running
    case .finished(let verdict, let flaky):
      state = .finished
      self.verdict = verdict
      self.flaky = flaky
      current = nil
    }
  }

  private func isOwn(_ file: String) -> Bool {
    test == nil || test == file
  }

  private mutating func startAttempt(_ number: Int) {
    attempt = number
    current = nil
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
