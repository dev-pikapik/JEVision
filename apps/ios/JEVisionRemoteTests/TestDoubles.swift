import Foundation
import LocalAuthentication
import Synchronization

@testable import JEVisionRemote

let macA = HostRecord(hostId: String(repeating: "a", count: 64), name: "Studio", deviceId: "d1", relayUrl: nil, addrs: [])

func pairingText(name: String = "Studio", exp: Int = 4_000_000_000) -> String {
  "jevision://pair?v=1&id=\(String(repeating: "a", count: 64))&name=\(name)&code=\(String(repeating: "0f", count: 16))&exp=\(exp)"
}

final class FakeSession: MacSession {
  let record: HostRecord
  let permission: Permission = .run
  let closed = Mutex(false)
  /// What following a run sends.
  private let events: AsyncThrowingStream<RunUpdate, any Error>

  init(record: HostRecord, events: AsyncThrowingStream<RunUpdate, any Error> = AsyncThrowingStream { $0.finish() }) {
    self.record = record
    self.events = events
  }

  func listProjects() async throws(RemoteError) -> [Project] { [Project(name: "Notes")] }
  func listTests(project: String) async throws(RemoteError) -> [TestEntry] { [] }
  func getTest(project: String, path: String) async throws(RemoteError) -> TestFlow { throw .other }
  func runTest(project: String, path: String, row: Int?) async throws(RemoteError) -> String { "run-1" }
  func stopRun(project: String, run: String) async throws(RemoteError) {}
  func editTest(project: String, path: String, revision: String, edit: TestEdit) async throws(RemoteError) -> EditedTest {
    throw .other
  }
  func runStatus(project: String, run: String) async throws(RemoteError) -> RunStatus { throw .other }
  func runEvents(project: String, run: String) -> AsyncThrowingStream<RunUpdate, any Error> { events }
  func listRuns(project: String) async throws(RemoteError) -> RunList { RunList(active: [], saved: []) }
  func report(project: String, run: String) async throws(RemoteError) -> RunReport { throw .other }
  func picture(project: String, run: String, name: String) async throws(RemoteError) -> Data { Data() }
  func close() async { closed.withLock { $0 = true } }
}

/// Answers with `failure` if set, otherwise with a session for the paired or asked-for Mac.
final class FakeClient: RemoteClient {
  let failure = Mutex<RemoteError?>(nil)
  let connects = Mutex(0)
  let pairs = Mutex(0)
  /// How long `connect` takes, so overlapping calls can be checked.
  let delay: Duration

  init(delay: Duration = .zero) { self.delay = delay }

  func pair(offer: String, deviceName: String) async throws(RemoteError) -> any MacSession {
    pairs.withLock { $0 += 1 }
    if let error = failure.withLock({ $0 }) { throw error }
    return FakeSession(record: macA)
  }

  func connect(_ host: HostRecord) async throws(RemoteError) -> any MacSession {
    connects.withLock { $0 += 1 }
    try? await Task.sleep(for: delay)
    if let error = failure.withLock({ $0 }) { throw error }
    var record = host
    record.addrs = ["192.168.1.20:52114"]
    return FakeSession(record: record)
  }
}

@MainActor
func makeLock(clock: Clock = Clock()) -> AppLock {
  AppLock(
    defaults: UserDefaults(suiteName: UUID().uuidString)!,
    now: { clock.now },
    authenticate: { _ in LAContext() })
}

@MainActor
func makeStore() -> HostStore {
  HostStore(url: URL.temporaryDirectory.appending(path: "\(UUID().uuidString)/PairedMacs.json"))
}

@MainActor
final class Clock {
  var now = Date(timeIntervalSince1970: 1_000_000)
  func advance(_ seconds: TimeInterval) { now += seconds }
}
