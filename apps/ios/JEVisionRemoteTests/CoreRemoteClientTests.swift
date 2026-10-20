import Foundation
import JEVisionMobile
import Testing

@testable import JEVisionRemote

struct CoreMappingTests {
  @Test(arguments: [
    (JEVisionMobile.RemoteError.PairingCodeInvalid, JEVisionRemote.RemoteError.invalidCode),
    (.PairingCodeUsed, .invalidCode),
    (.PairingLocked, .invalidCode),
    (.OfferMalformed, .invalidCode),
    (.PairingCodeExpired, .codeExpired),
    (.NotPaired, .revoked),
    (.Revoked, .revoked),
    (.HostOffline, .macOffline),
    (.HostNotRunning, .macOffline),
    (.Network, .macOffline),
    (.MacLocked, .macLocked),
    (.RunInProgress, .runInProgress),
    (.NotAllowed, .notAllowed),
    (.DeclinedOnMac, .declinedOnMac),
    (.DataRowNotFound, .dataRowNotFound),
    (.SignerFailed, .other),
    (.TestNotFound, .other),
  ])
  func errorsReadAsTheApps(core: JEVisionMobile.RemoteError, app: JEVisionRemote.RemoteError) {
    #expect(JEVisionRemote.RemoteError(core) == app)
  }

  @Test func readsTheMacsTimes() throws {
    #expect(try remoteDate("2026-10-18T12:00:00.250Z") == Date(timeIntervalSince1970: 1_792_324_800.25))
    #expect(try remoteDate("2026-10-18T12:00:00Z") == Date(timeIntervalSince1970: 1_792_324_800))
    #expect(throws: JEVisionRemote.RemoteError.other) { try remoteDate("yesterday") }
  }

  /// Paired Macs saved by earlier versions keep working, whichever side wrote them.
  @Test func savedMacsShareTheCoresFormat() throws {
    let record = JEVisionRemote.HostRecord(
      hostId: macA.hostId, name: "Studio", deviceId: "d1", relayUrl: "https://relay.example.com/",
      addrs: ["192.168.1.20:4433"])
    for record in [record, macA] {
      let saved = String(decoding: try JSONEncoder().encode(record), as: UTF8.self)
      #expect(JEVisionRemote.HostRecord(try hostRecordFromJson(json: saved)) == record)
      let core = Data(hostRecordToJson(record: record.wire).utf8)
      #expect(try JSONDecoder().decode(JEVisionRemote.HostRecord.self, from: core) == record)
    }
  }

  @Test func followsTheEventsTheScreensShow() {
    let started = JEVisionMobile.RunUpdate.event(
      event: .blockStarted(attempt: 1, seq: 1, test: "a.flow.json", block: "open", blockType: "openApp"))
    #expect(JEVisionRemote.RunUpdate(started) == .event(.blockStarted(test: "a.flow.json", block: "open", kind: "openApp")))
    let attemptEnd = JEVisionMobile.RunUpdate.event(event: .attemptFinished(attempt: 1, verdict: .failed, failure: nil))
    #expect(JEVisionRemote.RunUpdate(attemptEnd) == nil)
  }

  /// A newer Mac may send a verdict or a step status this version doesn't know; the core reads
  /// it as unknown and the screens show it as such instead of failing.
  @Test func unknownResultsFromANewerMacShowAsUnknown() {
    let step = JEVisionMobile.RunUpdate.event(
      event: .blockFinished(
        attempt: 1, seq: 1, test: "a.flow.json", block: "open", status: .unknown, durationMs: 5, failure: nil,
        branch: nil, branches: nil, branchCondition: nil))
    #expect(
      JEVisionRemote.RunUpdate(step) == .event(.blockFinished(test: "a.flow.json", block: "open", status: .unknown, failure: nil)))
    let end = JEVisionMobile.RunUpdate.event(event: .finished(verdict: .unknown, flaky: false))
    #expect(JEVisionRemote.RunUpdate(end) == .event(.finished(verdict: .unknown, flaky: false)))
    #expect(VerdictStyle(.unknown).title == String(localized: "Unknown status"))
    #expect(VerdictStyle(StepStatus.unknown).title == String(localized: "Unknown status"))
  }

  /// A test that didn't run because a test it depends on failed reads as blocked and names it.
  @Test func aBlockedRunNamesTheTestItDependsOn() throws {
    let report = try RunReport(
      JEVisionMobile.RunReport(
        test: "Checkout", file: "checkout.flow.json", app: "com.example.shop", startedAt: "2026-10-17T11:00:00.000Z",
        durationMs: 0, verdict: .blocked, flaky: false, baselinesCreated: [], attempts: [], requestedBy: nil,
        blockedBy: "Sign in"))
    #expect(report.verdict == .blocked)
    #expect(report.blockedBy == "Sign in")
    #expect(VerdictStyle(report.verdict).title == String(localized: "Blocked"))
    #expect(RunText.outcome(report) == String(localized: "Blocked: \("Sign in") didn’t pass. The test didn’t run."))
  }
}

/// Talks to a real Mac. Turn on pairing in JEVision on the Mac, copy the pairing link and run
/// `TEST_RUNNER_JEVISION_REMOTE_E2E=1 TEST_RUNNER_JEVISION_REMOTE_OFFER='jevision://pair?…' xcodebuild test …`.
/// `cargo run -p jevision-remote --example test-host` stands in for the Mac. The build must be
/// signed, even ad hoc, for the device key to reach the keychain.
@Suite(.enabled(if: ProcessInfo.processInfo.environment["JEVISION_REMOTE_E2E"] == "1"))
struct RemoteEndToEndTests {
  @Test func pairsBrowsesAndSignsInAgain() async throws {
    let offer = try #require(ProcessInfo.processInfo.environment["JEVISION_REMOTE_OFFER"])
    let link = try #require(PairingLink(offer))
    let client = CoreRemoteClient(signer: DeviceKey())

    let paired = try await client.pair(offer: link.text, deviceName: "JEVision end-to-end test")
    var record = paired.record
    // A Mac offered on loopback, as `test-host` is, may not be reachable at the network
    // addresses it reports afterwards, so sign in again where the code pointed.
    let offered = URLComponents(string: offer)?.queryItems?.filter { $0.name == "addr" }.compactMap(\.value) ?? []
    if !offered.isEmpty, offered.allSatisfy({ $0.hasPrefix("127.0.0.1:") }) { record.addrs = offered }
    #expect(record.name == link.macName)
    let projects = try await paired.listProjects()
    let project = try #require(projects.first)
    let tests = try await paired.listTests(project: project.name)
    if let test = tests.first(where: { !$0.isFolder }) {
      let flow = try await paired.getTest(project: project.name, path: test.path)
      #expect(!flow.blocks.isEmpty)
    }
    let runs = try await paired.listRuns(project: project.name)
    if let saved = runs.saved.first {
      let report = try await paired.report(project: project.name, run: saved.run)
      #expect(report.file == saved.file)
    }
    if ProcessInfo.processInfo.environment["JEVISION_REMOTE_E2E_RUN"] == "1" {
      try await runsOneRow(of: tests, in: project.name, on: paired)
    }
    await paired.close()

    let again = try await client.connect(record)
    #expect(try await again.listProjects() == projects)
    await again.close()
  }

  /// Runs the second row of the first test with a data table, follows it live and reads its
  /// report. It drives the Mac, so it runs only with `TEST_RUNNER_JEVISION_REMOTE_E2E_RUN=1`;
  /// `cargo run -p jevision-remote --example test-host` plays runs without driving anything.
  private func runsOneRow(of tests: [JEVisionRemote.TestEntry], in project: String, on mac: any JEVisionRemote.MacSession) async throws {
    var table: (path: String, rows: [String])?
    for test in tests where !test.isFolder {
      let flow = try await mac.getTest(project: project, path: test.path)
      if flow.rows.count > 1 {
        table = (test.path, flow.rows)
        break
      }
    }
    let (path, rows) = try #require(table, "the project has a test with a data table of two rows or more")
    let run = try await mac.runTest(project: project, path: path, row: 1)
    var blocks = 0
    var verdict: JEVisionRemote.Verdict?
    for try await update in mac.runEvents(project: project, run: run) {
      switch update {
      case .event(.blockStarted): blocks += 1
      case .event(.finished(let end, _)): verdict = end
      default: break
      }
    }
    #expect(blocks > 0)
    let report = try await mac.report(project: project, run: run)
    #expect(report.verdict == verdict)
    #expect(report.attempts.map(\.row) == [JEVisionRemote.RunReport.DataRow(number: 2, label: rows[1])])
    #expect(try await mac.listRuns(project: project).saved.contains { $0.run == run })
  }
}
