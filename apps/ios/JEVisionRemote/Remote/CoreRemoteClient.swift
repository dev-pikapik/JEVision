import CoreGraphics
import Foundation
import JEVisionMobile

/// The network layer: the Rust core's remote client, signing in with `DeviceKey`. This is the
/// only file that sees the core's types; everything it hands out is the app's own.
struct CoreRemoteClient: RemoteClient {
  let signer: DeviceKey

  /// No mDNS: on an iPhone it needs the multicast entitlement, which Apple grants per team.
  /// The addresses from the pairing code, refreshed on every sign-in, the address lookup
  /// service and the relay reach the Mac without it.
  static let network = Network(relay: .default, localDiscovery: false, globalDiscovery: true)

  func pair(offer: String, deviceName: String) async throws(RemoteError) -> any MacSession {
    try await open {
      let offer = try parsePairingOffer(text: offer)
      return try await RemoteConnection.pair(
        offer: offer, deviceName: deviceName, signer: signer, network: Self.network)
    }
  }

  func connect(_ host: HostRecord) async throws(RemoteError) -> any MacSession {
    try await open {
      try await RemoteConnection.connect(record: host.wire, signer: signer, network: Self.network)
    }
  }

  private func open(_ body: () async throws -> RemoteConnection) async throws(RemoteError) -> any MacSession {
    // The core asks for the public key on its network threads and expects it at hand.
    do { _ = try signer.loadPublicKey() } catch { throw .other }
    return CoreMacSession(try await core(body))
  }
}

/// One signed-in `RemoteConnection`. The core reconnects it on its own after network changes.
final class CoreMacSession: MacSession {
  private let connection: RemoteConnection

  init(_ connection: RemoteConnection) {
    self.connection = connection
  }

  var record: HostRecord { HostRecord(connection.record()) }

  var permission: Permission {
    switch connection.permission() {
    case .view: .view
    case .run: .run
    }
  }

  func listProjects() async throws(RemoteError) -> [Project] {
    try await core { try await connection.listProjects().map { Project(name: $0.name) } }
  }

  func listTests(project: String) async throws(RemoteError) -> [TestEntry] {
    try await core { try await connection.listTests(project: project).map(TestEntry.init) }
  }

  func getTest(project: String, path: String) async throws(RemoteError) -> TestFlow {
    try await core { TestFlow(try await connection.getTest(project: project, path: path)) }
  }

  func runTest(project: String, path: String, row: Int?) async throws(RemoteError) -> String {
    try await core { try await connection.runTest(project: project, path: path, row: row.map(UInt32.init)) }
  }

  func stopRun(project: String, run: String) async throws(RemoteError) {
    try await core { try await connection.stopRun(project: project, run: run) }
  }

  func runStatus(project: String, run: String) async throws(RemoteError) -> RunStatus {
    try await core { RunStatus(try await connection.runStatus(project: project, run: run)) }
  }

  func runEvents(project: String, run: String) -> AsyncThrowingStream<RunUpdate, any Error> {
    AsyncThrowingStream { continuation in
      let task = Task { [connection] in
        do {
          let watcher = try await core { try await connection.watch(project: project, run: run) }
          // A pending `next()` finishes in the core regardless; once cancelled, the loop stops
          // asking and the watcher is released with the task.
          while !Task.isCancelled, let update = try await core({ try await watcher.next() }) {
            if let update = RunUpdate(update) { continuation.yield(update) }
          }
          continuation.finish()
        } catch {
          continuation.finish(throwing: error)
        }
      }
      continuation.onTermination = { _ in task.cancel() }
    }
  }

  func listRuns(project: String) async throws(RemoteError) -> RunList {
    try await core { try RunList(try await connection.listRuns(project: project)) }
  }

  func report(project: String, run: String) async throws(RemoteError) -> RunReport {
    try await core { try RunReport(try await connection.report(project: project, run: run)) }
  }

  func picture(project: String, run: String, name: String) async throws(RemoteError) -> Data {
    try await core { try await connection.picture(project: project, run: run, picture: name) }
  }

  func close() async {
    await connection.close()
  }
}

extension DeviceKey: DeviceSigner {
  func publicKey() -> Data {
    // Empty only if the key can't be read at all; `CoreRemoteClient` reads it before it
    // connects and reports that failure itself.
    (try? loadPublicKey()) ?? Data()
  }

  func sign(message: Data) throws -> Data {
    do { return try sign(message) } catch { throw JEVisionMobile.RemoteError.SignerFailed }
  }
}

/// Runs a call into the core, turning its errors into the app's.
private func core<T>(_ body: () async throws -> T) async throws(RemoteError) -> T {
  do {
    return try await body()
  } catch let error as JEVisionMobile.RemoteError {
    throw RemoteError(error)
  } catch let error as RemoteError {
    throw error
  } catch {
    throw .other
  }
}

extension RemoteError {
  init(_ error: JEVisionMobile.RemoteError) {
    switch error {
    case .PairingCodeInvalid, .PairingCodeUsed, .PairingLocked, .OfferWrongScheme, .OfferMalformed:
      self = .invalidCode
    case .PairingCodeExpired: self = .codeExpired
    case .NotPaired, .Revoked: self = .revoked
    case .NotAllowed: self = .notAllowed
    case .MacLocked: self = .macLocked
    case .RunInProgress: self = .runInProgress
    case .DeclinedOnMac: self = .declinedOnMac
    case .DataRowNotFound: self = .dataRowNotFound
    case .HostOffline, .HostNotRunning, .Network: self = .macOffline
    case .BadSignature, .ProjectNotFound, .TestNotFound, .TestUnreadable, .RunNotFound, .PictureNotFound,
      .PathOutsideProject, .TooLarge, .UnexpectedMessage, .InvalidSecretKey, .InvalidRelayUrl, .SignerFailed,
      .Storage, .HostFailed:
      self = .other
    }
  }
}

// MARK: - Core types to app types

/// Times from the Mac, RFC 3339 in UTC, such as `2026-10-18T12:00:00.000Z`.
func remoteDate(_ text: String) throws(RemoteError) -> Date {
  if let date = try? Date.ISO8601FormatStyle(includingFractionalSeconds: true).parse(text) { return date }
  if let date = try? Date.ISO8601FormatStyle().parse(text) { return date }
  throw .other
}

extension HostRecord {
  init(_ record: JEVisionMobile.HostRecord) {
    self.init(
      hostId: record.hostId, name: record.name, deviceId: record.deviceId, relayUrl: record.relayUrl,
      addrs: record.addrs)
  }

  var wire: JEVisionMobile.HostRecord {
    JEVisionMobile.HostRecord(hostId: hostId, name: name, deviceId: deviceId, relayUrl: relayUrl, addrs: addrs)
  }
}

extension Verdict {
  init(_ verdict: JEVisionMobile.Verdict) {
    switch verdict {
    case .passed: self = .passed
    case .failed: self = .failed
    case .cancelled: self = .cancelled
    case .blocked: self = .blocked
    case .unknown: self = .unknown
    }
  }
}

extension StepStatus {
  init(_ status: JEVisionMobile.StepStatus) {
    switch status {
    case .passed: self = .passed
    case .failed: self = .failed
    case .baselineCreated: self = .baselineCreated
    case .cancelled: self = .cancelled
    case .unknown: self = .unknown
    }
  }
}

extension Failure {
  init(_ failure: JEVisionMobile.Failure) {
    self.init(code: failure.code, params: failure.params)
  }
}

extension TestEntry {
  init(_ entry: JEVisionMobile.TestEntry) {
    self.init(
      path: entry.path, name: entry.name, isFolder: entry.isFolder,
      lastRun: entry.lastRun.flatMap { last in
        (try? remoteDate(last.startedAt)).map { LastRun(run: last.run, verdict: Verdict(last.verdict), startedAt: $0) }
      })
  }
}

extension CGRect {
  init(_ area: Area) {
    self.init(x: area.x, y: area.y, width: area.width, height: area.height)
  }
}

extension TestFlow {
  init(_ test: TestSummary) {
    self.init(
      name: test.name, description: test.description, app: test.app, start: test.start,
      size: CGSize(width: test.width, height: test.height),
      blocks: test.blocks.map { Block(id: $0.id, kind: $0.kind, title: $0.title, area: $0.area.map(CGRect.init)) },
      frames: test.frames.map { Frame(block: $0.block, area: CGRect($0.area)) },
      edges: test.edges.map {
        Edge(from: $0.from, edge: $0.edge, to: $0.to, points: $0.points.map { CGPoint(x: $0.x, y: $0.y) })
      },
      rows: test.rows, priority: test.priority, testType: test.testType, status: test.status, author: test.author)
  }
}

extension RunStatus {
  init(_ status: JEVisionMobile.RunStatus) {
    let state: RunState =
      switch status.state {
      case .running: .running
      case .paused: .paused
      case .finished: .finished
      }
    self.init(
      run: status.run, test: status.test, state: state, verdict: status.verdict.map(Verdict.init),
      attempt: Int(status.attempt), block: status.block,
      waiting: status.waiting.map { wait in
        ScreenWait(
          holder: wait.holder.map {
            ScreenHolder(process: $0.process, test: $0.test, since: Date(timeIntervalSince1970: TimeInterval($0.since)))
          })
      })
  }
}

extension RunList {
  init(_ list: JEVisionMobile.RunList) throws(RemoteError) {
    var saved: [RunSummary] = []
    for run in list.saved {
      saved.append(
        RunSummary(
          run: run.run, test: run.test, file: run.file, startedAt: try remoteDate(run.startedAt),
          durationMs: run.durationMs, verdict: Verdict(run.verdict), flaky: run.flaky))
    }
    self.init(active: list.active.map(RunStatus.init), saved: saved)
  }
}

extension RunUpdate {
  /// `nil` for what the app doesn't follow: the end of an attempt shows in its blocks and in
  /// the next `attemptStarted` or `finished`.
  init?(_ update: JEVisionMobile.RunUpdate) {
    switch update {
    case .status(let status):
      self = .status(RunStatus(status))
    case .event(let event):
      switch event {
      case .attemptStarted(let attempt):
        self = .event(.attemptStarted(attempt: Int(attempt)))
      case .blockStarted(_, _, let test, let block, let kind):
        self = .event(.blockStarted(test: test, block: block, kind: kind))
      case .blockFinished(_, _, let test, let block, let status, _, let failure, let branch, let branches, let condition):
        self = .event(
          .blockFinished(
            test: test, block: block, status: StepStatus(status), failure: failure.map(Failure.init), branch: branch,
            branches: branches.map(Int.init), branchCondition: condition))
      case .paused(let block, _):
        self = .event(.paused(block: block))
      case .resumed:
        self = .event(.resumed)
      case .attemptFinished:
        return nil
      case .finished(let verdict, let flaky):
        self = .event(.finished(verdict: Verdict(verdict), flaky: flaky))
      }
    }
  }
}

extension RunReport {
  init(_ report: JEVisionMobile.RunReport) throws(RemoteError) {
    self.init(
      test: report.test, file: report.file, app: report.app, startedAt: try remoteDate(report.startedAt),
      durationMs: report.durationMs, verdict: Verdict(report.verdict), flaky: report.flaky,
      baselinesCreated: report.baselinesCreated,
      attempts: report.attempts.map { attempt in
        Attempt(
          number: Int(attempt.number), verdict: Verdict(attempt.verdict), durationMs: attempt.durationMs,
          failure: attempt.failure.map(Failure.init),
          row: attempt.row.map { DataRow(number: Int($0.number), label: $0.label) },
          steps: attempt.steps.map { step in
            Step(
              seq: Int(step.seq), test: step.test, block: step.block, kind: step.blockType,
              status: StepStatus(step.status), durationMs: step.durationMs, before: step.before,
              after: step.after, expected: step.expected, diff: step.diff, failure: step.failure.map(Failure.init),
              branch: step.branch, branches: step.branches.map(Int.init), branchCondition: step.branchCondition)
          })
      },
      requestedBy: report.requestedBy, blockedBy: report.blockedBy)
  }
}
