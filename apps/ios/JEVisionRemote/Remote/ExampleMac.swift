import Foundation
import Synchronization

/// A Mac built into the app, for someone who wants to look around before pairing one. It has
/// one project with a few tests and some history, and plays the runs started on it. Nothing in
/// it touches the network or is saved.
final class ExampleClient: RemoteClient {
  static var mac: HostRecord {
    HostRecord(hostId: "example", name: String(localized: "Example Mac"), deviceId: "example", relayUrl: nil, addrs: [])
  }

  private let session: ExampleSession

  /// `deviceName` is who the reports of runs started here say asked for them.
  /// `step` is how long each block of a run takes.
  init(deviceName: String, now: Date = Date(), step: TimeInterval = 0.8) {
    session = ExampleSession(deviceName: deviceName, now: now, step: step)
  }

  /// The example is the only Mac this client knows; a real one is paired outside it.
  func pair(offer: String, deviceName: String) async throws(RemoteError) -> any MacSession {
    throw .notAllowed
  }

  func connect(_ host: HostRecord) async throws(RemoteError) -> any MacSession {
    guard host.id == Self.mac.id else { throw .macOffline }
    return session
  }
}

final class ExampleSession: MacSession {
  /// One run of the example. What happens in it follows from the test, the row and the time
  /// since it started, so a run plays on whether or not a screen follows it.
  struct Run: Sendable {
    var id: String
    var path: String
    var row: Int?
    var startedAt: Date
    var requestedBy: String?
    /// How long after the start someone stopped it.
    var stoppedAfter: TimeInterval?
  }

  private struct Timed {
    var at: TimeInterval
    var event: RunEvent
  }

  var record: HostRecord { ExampleClient.mac }
  let permission = Permission.run

  private let deviceName: String
  private let step: TimeInterval
  private let runs: Mutex<[Run]>

  init(deviceName: String, now: Date, step: TimeInterval) {
    self.deviceName = deviceName
    self.step = step
    // History from before: yesterday's sign-in failed at its last check, the greetings passed.
    runs = Mutex([
      Run(id: "example-2", path: ExampleProject.signIn, startedAt: now.addingTimeInterval(-26 * 3600)),
      Run(id: "example-1", path: ExampleProject.people, startedAt: now.addingTimeInterval(-50 * 3600)),
    ])
  }

  func listProjects() async throws(RemoteError) -> [Project] {
    [Project(name: ExampleProject.name)]
  }

  func listTests(project: String) async throws(RemoteError) -> [TestEntry] {
    try check(project)
    let finished = finishedRuns()
    return ExampleProject.entries().map { entry in
      var entry = entry
      entry.lastRun = finished.first { $0.file == entry.path }.map {
        LastRun(run: $0.run, verdict: $0.verdict, startedAt: $0.startedAt)
      }
      return entry
    }
  }

  func getTest(project: String, path: String) async throws(RemoteError) -> TestFlow {
    try check(project)
    guard let flow = ExampleProject.flow(path) else { throw .other }
    return flow
  }

  func runTest(project: String, path: String, row: Int?) async throws(RemoteError) -> String {
    try check(project)
    guard let flow = ExampleProject.flow(path) else { throw .other }
    if let row, !flow.rows.indices.contains(row) { throw .dataRowNotFound }
    let now = Date()
    return try runs.withLock { runs throws(RemoteError) in
      if runs.contains(where: { !isFinished($0, at: now) }) { throw .runInProgress }
      let run = Run(id: "example-\(runs.count + 1)", path: path, row: row, startedAt: now, requestedBy: deviceName)
      runs.insert(run, at: 0)
      return run.id
    }
  }

  func stopRun(project: String, run: String) async throws(RemoteError) {
    try check(project)
    let now = Date()
    try runs.withLock { runs throws(RemoteError) in
      guard let index = runs.firstIndex(where: { $0.id == run }) else { throw .other }
      if !isFinished(runs[index], at: now) {
        runs[index].stoppedAfter = now.timeIntervalSince(runs[index].startedAt)
      }
    }
  }

  func runStatus(project: String, run: String) async throws(RemoteError) -> RunStatus {
    try check(project)
    let run = try find(run)
    return status(of: run, events: events(of: run).prefix { $0.at <= Date().timeIntervalSince(run.startedAt) })
  }

  func runEvents(project: String, run id: String) -> AsyncThrowingStream<RunUpdate, any Error> {
    AsyncThrowingStream { continuation in
      let task = Task {
        var sent: Int?
        while !Task.isCancelled {
          let run: Run
          do { run = try find(id) } catch { return continuation.finish(throwing: error) }
          let events = events(of: run)
          let due = events.prefix { $0.at <= Date().timeIntervalSince(run.startedAt) }
          if let sent {
            for timed in due.dropFirst(sent) { continuation.yield(.event(timed.event)) }
          } else {
            continuation.yield(.status(status(of: run, events: due)))
          }
          sent = due.count
          if due.count == events.count { break }
          try? await Task.sleep(for: .milliseconds(100))
        }
        continuation.finish()
      }
      continuation.onTermination = { _ in task.cancel() }
    }
  }

  func listRuns(project: String) async throws(RemoteError) -> RunList {
    try check(project)
    let now = Date()
    let active = runs.withLock { $0 }.filter { !isFinished($0, at: now) }.map { run in
      status(of: run, events: events(of: run).prefix { $0.at <= now.timeIntervalSince(run.startedAt) })
    }
    return RunList(active: active, saved: finishedRuns())
  }

  func report(project: String, run: String) async throws(RemoteError) -> RunReport {
    try check(project)
    let run = try find(run)
    guard isFinished(run, at: Date()), let flow = ExampleProject.flow(run.path) else { throw .other }
    let events = events(of: run).map(\.event)
    var attempts: [RunReport.Attempt] = []
    var kinds: [String: String] = [:]
    let rows = rows(of: run, in: flow)
    let ms = UInt64(step * 1000)
    for event in events {
      switch event {
      case .attemptStarted(let number):
        let row = rows[attempts.count].map { RunReport.DataRow(number: $0 + 1, label: flow.rows[$0]) }
        attempts.append(.init(number: number, verdict: .passed, durationMs: 0, row: row, steps: []))
      case .blockStarted(_, let block, let kind, _):
        kinds[block] = kind
      case .blockFinished(let test, let block, let status, let failure, let branch, let branches, let condition):
        let picture = status == .failed ? block : nil
        attempts[attempts.count - 1].steps.append(
          .init(
            seq: attempts[attempts.count - 1].steps.count + 1, test: test, block: block, kind: kinds[block] ?? "",
            status: status, durationMs: ms, before: nil, after: picture.map { "\($0)-after.png" },
            expected: picture.map { "\($0)-expected.png" }, diff: nil, failure: failure, branch: branch,
            branches: branches, branchCondition: condition))
        attempts[attempts.count - 1].durationMs += ms
        if let failure {
          attempts[attempts.count - 1].verdict = .failed
          attempts[attempts.count - 1].failure = failure
        }
      case .finished(.cancelled, _) where !attempts.isEmpty:
        attempts[attempts.count - 1].verdict = .cancelled
      case .finished, .paused, .resumed:
        break
      }
    }
    return RunReport(
      test: flow.name, file: run.path, app: flow.app, startedAt: run.startedAt,
      durationMs: attempts.reduce(0) { $0 + $1.durationMs }, verdict: verdict(of: events), flaky: false,
      baselinesCreated: [], attempts: attempts, requestedBy: run.requestedBy)
  }

  func picture(project: String, run: String, name: String) async throws(RemoteError) -> Data {
    try check(project)
    guard let picture = ExampleProject.picture(name) else { throw .other }
    return picture
  }

  /// Nothing to release: the example has no connection.
  /// The example Mac lets this iPhone look and run, not change its tests.
  func editTest(project: String, path: String, revision: String, edit: TestEdit) async throws(RemoteError) -> EditedTest {
    throw .notAllowed
  }

  func close() async {}

  private func check(_ project: String) throws(RemoteError) {
    guard project == ExampleProject.name else { throw .other }
  }

  private func find(_ id: String) throws(RemoteError) -> Run {
    guard let run = runs.withLock({ $0.first { $0.id == id } }) else { throw .other }
    return run
  }

  private func finishedRuns() -> [RunSummary] {
    let now = Date()
    return runs.withLock { $0 }.filter { isFinished($0, at: now) }.compactMap { run in
      guard let flow = ExampleProject.flow(run.path) else { return nil }
      let events = events(of: run)
      return RunSummary(
        run: run.id, test: flow.name, file: run.path, startedAt: run.startedAt,
        durationMs: UInt64((events.last?.at ?? 0) * 1000), verdict: verdict(of: events.map(\.event)), flaky: false)
    }
  }

  private func isFinished(_ run: Run, at now: Date) -> Bool {
    (events(of: run).last?.at ?? 0) <= now.timeIntervalSince(run.startedAt)
  }

  /// Every row the run goes through, as indexes into `TestFlow.rows`; one `nil` without a table.
  private func rows(of run: Run, in flow: TestFlow) -> [Int?] {
    if let row = run.row { [row] } else if flow.rows.isEmpty { [nil] } else { flow.rows.indices.map(Optional.some) }
  }

  /// What happens in the run and when, counted from its start: each row tries every block in
  /// turn, and the run ends at its first failure or when it is stopped.
  private func events(of run: Run) -> [Timed] {
    guard let flow = ExampleProject.flow(run.path) else { return [] }
    var events: [Timed] = []
    var time: TimeInterval = 0
    var verdict = Verdict.passed
    rows: for row in rows(of: run, in: flow) {
      events.append(Timed(at: time, event: .attemptStarted(attempt: 1)))
      for block in flow.blocks {
        let title = ExampleProject.runningTitle(of: block, row: row.map { flow.rows[$0] })
        events.append(Timed(at: time, event: .blockStarted(test: run.path, block: block.id, kind: block.kind, title: title)))
        time += step
        let failure = ExampleProject.failure(of: block, in: run.path)
        let branch = ExampleProject.branch(of: block, in: run.path)
        events.append(
          Timed(
            at: time,
            event: .blockFinished(
              test: run.path, block: block.id, status: failure == nil ? .passed : .failed, failure: failure,
              branch: branch.map { _ in "then" }, branches: branch.map { _ in 2 }, branchCondition: branch)))
        if failure != nil {
          verdict = .failed
          break rows
        }
      }
    }
    events.append(Timed(at: time, event: .finished(verdict: verdict, flaky: false)))
    guard let stop = run.stoppedAfter, stop < time else { return events }
    return events.prefix { $0.at <= stop } + [Timed(at: stop, event: .finished(verdict: .cancelled, flaky: false))]
  }

  private func status(of run: Run, events: some Sequence<Timed>) -> RunStatus {
    var status = RunStatus(run: run.id, test: run.path, state: .running, verdict: nil, attempt: 1, block: nil)
    for timed in events {
      switch timed.event {
      case .attemptStarted(let attempt): status.attempt = attempt
      case .blockStarted(_, let block, _, _): status.block = block
      case .blockFinished: status.block = nil
      case .finished(let verdict, _):
        status.state = .finished
        status.verdict = verdict
      case .paused, .resumed: break
      }
    }
    return status
  }

  private func verdict(of events: [RunEvent]) -> Verdict {
    guard case .finished(let verdict, _) = events.last else { return .unknown }
    return verdict
  }
}
