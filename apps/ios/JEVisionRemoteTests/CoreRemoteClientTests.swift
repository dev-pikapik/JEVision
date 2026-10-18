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
}

/// Talks to a real Mac. Turn on pairing in JEVision on the Mac, copy the pairing link and run
/// `TEST_RUNNER_JEVISION_REMOTE_E2E=1 TEST_RUNNER_JEVISION_REMOTE_OFFER='jevision://pair?…' xcodebuild test …`.
@Suite(.enabled(if: ProcessInfo.processInfo.environment["JEVISION_REMOTE_E2E"] == "1"))
struct RemoteEndToEndTests {
  @Test func pairsBrowsesAndSignsInAgain() async throws {
    let offer = try #require(ProcessInfo.processInfo.environment["JEVISION_REMOTE_OFFER"])
    let link = try #require(PairingLink(offer))
    let client = CoreRemoteClient(signer: DeviceKey())

    let paired = try await client.pair(offer: link.text, deviceName: "JEVision end-to-end test")
    let record = paired.record
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
    await paired.close()

    let again = try await client.connect(record)
    #expect(try await again.listProjects() == projects)
    await again.close()
  }
}
