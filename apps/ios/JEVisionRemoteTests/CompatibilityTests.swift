import Foundation
import JEVisionMobile
import Testing

@testable import JEVisionRemote

/// The paired Macs as saved by older and newer versions of the app, and files that can't be read.
@MainActor
struct HostStoreTests {
  let store = makeStore()
  let macB = String(repeating: "b", count: 64)

  private func write(_ json: String) throws {
    try FileManager.default.createDirectory(
      at: store.url.deletingLastPathComponent(), withIntermediateDirectories: true)
    try Data(json.utf8).write(to: store.url)
  }

  private func saved() throws -> [String: Any] {
    try #require(try JSONSerialization.jsonObject(with: Data(contentsOf: store.url)) as? [String: Any])
  }

  private func savedMacs() throws -> [[String: Any]] {
    try #require(try saved()["macs"] as? [[String: Any]])
  }

  /// Takes away or gives back the right to read the file, as data protection does before the
  /// iPhone's first unlock.
  private func setReadable(_ readable: Bool) throws {
    try FileManager.default.setAttributes(
      [.posixPermissions: readable ? 0o644 : 0o000], ofItemAtPath: store.url.path)
  }

  private func mac(_ id: String, _ extra: String = "") -> String {
    #"{"hostId":"\#(id)","name":"Studio","deviceId":"d1","addrs":[]\#(extra)}"#
  }

  @Test func aListFromTheFirstVersionsLoadsAndIsSavedInTheNewShape() throws {
    try write("[\(mac(macA.hostId))]")
    let loaded = store.load()
    #expect(loaded.hosts == [macA])
    #expect(loaded.problem == nil)

    try store.save(loaded.hosts)

    #expect(try saved()["format"] as? Int == 1)
    #expect(try savedMacs().map { $0["hostId"] as? String } == [macA.hostId])
    #expect(store.load().hosts == [macA])
  }

  @Test func whatThisVersionDoesntKnowSurvivesASave() throws {
    try write(#"{"format":1,"sync":{"on":true},"macs":[\#(mac(macA.hostId, #","color":"blue""#))]}"#)
    var host = try #require(store.load().hosts.first)
    host.addrs = ["192.168.1.20:4433"]

    try store.save([host])

    #expect((try saved()["sync"] as? [String: Bool]) == ["on": true])
    let saved = try #require(try savedMacs().first)
    #expect(saved["color"] as? String == "blue")
    #expect(saved["addrs"] as? [String] == ["192.168.1.20:4433"])
  }

  @Test func aMacRemovedInTheAppIsRemovedFromTheFile() throws {
    try write(#"{"format":1,"macs":[\#(mac(macA.hostId)),\#(mac(macB))]}"#)
    #expect(store.load().hosts.count == 2)

    try store.save([macA])

    #expect(try savedMacs().map { $0["hostId"] as? String } == [macA.hostId])
  }

  @Test func aRelayThatIsGoneIsRemovedFromTheFile() throws {
    try write(#"{"format":1,"macs":[\#(mac(macA.hostId, #","relayUrl":"https://relay.example.com/""#))]}"#)
    #expect(store.load().hosts.first?.relayUrl == "https://relay.example.com/")

    try store.save([macA])

    let saved = try #require(try savedMacs().first)
    #expect(saved["relayUrl"] == nil)
    #expect(store.load().hosts == [macA])
  }

  @Test func aMacThisVersionCantReadStaysInTheFileAndIsNotShown() throws {
    let unreadable = #"{"hostId":"\#(macB)","name":"Office","addrs":"later"}"#
    try write(#"{"format":1,"macs":[\#(mac(macA.hostId)),\#(unreadable)]}"#)
    #expect(store.load().hosts == [macA])

    try store.save([macA])

    let kept = try JSONSerialization.jsonObject(with: Data(unreadable.utf8)) as? NSDictionary
    #expect(try savedMacs().map { $0 as NSDictionary }.contains { $0 == kept })
    #expect(store.load().hosts == [macA])
  }

  @Test(arguments: [
    #"{"format":2,"macs":[MAC],"groups":[]}"#,
    #"{"format":1,"minVersion":"999.0.0","macs":[MAC]}"#,
    #"{"format":1,"minVersion":"soon","macs":[MAC]}"#,
    #"{"format":1,"minVersion":3,"macs":[MAC]}"#,
  ])
  func aListFromANewerVersionIsReadButNeverWritten(_ json: String) throws {
    try write(json.replacingOccurrences(of: "MAC", with: mac(macA.hostId)))
    let before = try Data(contentsOf: store.url)
    #expect(store.load().hosts == [macA])

    #expect(throws: HostStore.NewerList.self) { try store.save([]) }
    #expect(try Data(contentsOf: store.url) == before)
  }

  @Test func aNewerListSaysWhyTheAppCantChangeIt() async throws {
    try write(#"{"format":2,"macs":[\#(mac(macA.hostId))]}"#)
    let before = try Data(contentsOf: store.url)
    let model = AppModel(client: FakeClient(), store: store, lock: makeLock())
    #expect(model.hosts == [macA])

    await model.forget(macA)

    #expect(model.storageProblem == .newerList)
    #expect(try Data(contentsOf: store.url) == before)
  }

  @Test func anOlderMinimumVersionStillSaves() throws {
    try write(#"{"format":1,"minVersion":"0.0.1","macs":[\#(mac(macA.hostId))]}"#)

    try store.save([])

    #expect(try saved()["minVersion"] as? String == "0.0.1")
    #expect(try savedMacs().isEmpty)
  }

  @Test func aNewerListInAnotherShapeIsLeftAlone() throws {
    try write(#"{"format":3,"macs":{"studio":{}}}"#)
    let before = try Data(contentsOf: store.url)
    let loaded = store.load()
    #expect(loaded.hosts.isEmpty)
    #expect(loaded.problem == nil)

    #expect(throws: HostStore.NewerList.self) { try store.save([macA]) }
    #expect(try Data(contentsOf: store.url) == before)
  }

  @Test func aPreReleaseOfThisVersionIsNotNewer() throws {
    // The lowest pre-release of this app's version: never later than the app.
    let release = appVersion().prefix { $0 != "-" && $0 != "+" }
    try write(#"{"format":1,"minVersion":"\#(release)-0.2","macs":[\#(mac(macA.hostId))]}"#)

    try store.save([])

    #expect(try savedMacs().isEmpty)
  }

  @Test(arguments: [
    "not a list", #""a string""#, "42", #"{"format":2.5,"macs":[]}"#, #"{"format":"2","macs":[]}"#,
    #"{"format":0,"macs":[]}"#, #"{"format":true,"macs":[]}"#, #"{"format":1}"#, #"{"format":1,"macs":{}}"#,
  ])
  func anUnreadableListIsKeptAsideAndTheAppSaysSo(_ contents: String) async throws {
    try write(contents)
    try Data("an older copy".utf8).write(to: store.unreadableURL)

    let model = AppModel(client: FakeClient(), store: store, lock: makeLock())

    #expect(model.hosts.isEmpty)
    #expect(model.listWasUnreadable)
    #expect(try Data(contentsOf: store.unreadableURL) == Data(contents.utf8))
    #expect(!FileManager.default.fileExists(atPath: store.url.path))
    #expect(!AppModel(client: FakeClient(), store: store, lock: makeLock()).listWasUnreadable)
  }

  @Test func aListThatCantBeOpenedIsNeitherMovedNorWrittenOver() throws {
    try write(#"{"format":1,"macs":[\#(mac(macB))]}"#)
    let before = try Data(contentsOf: store.url)
    try setReadable(false)

    let loaded = store.load()
    #expect(loaded.hosts.isEmpty)
    #expect(loaded.problem == .notOpened)
    #expect(throws: HostStore.NotOpened.self) { try store.save([macA]) }

    try setReadable(true)
    #expect(!FileManager.default.fileExists(atPath: store.unreadableURL.path))
    #expect(try Data(contentsOf: store.url) == before)
  }

  @Test func theAppNeverWritesOverAListItCouldntOpen() async throws {
    try write(#"{"format":1,"macs":[\#(mac(macB))]}"#)
    let before = try Data(contentsOf: store.url)
    try setReadable(false)
    let model = AppModel(client: FakeClient(), store: store, lock: makeLock())
    #expect(model.hosts.isEmpty)
    #expect(model.listDidNotOpen)
    #expect(!model.listWasUnreadable)
    try setReadable(true)

    _ = try await model.pair(try #require(PairingLink(pairingText())), deviceName: "iPhone")

    #expect(model.hosts == [macA])
    #expect(model.storageProblem == .notOpened)
    #expect(try Data(contentsOf: store.url) == before)
  }

  @Test func theListLoadsOnceItOpensAndKeepsMacsPairedMeanwhile() async throws {
    try write(#"{"format":1,"macs":[\#(mac(macB))]}"#)
    try setReadable(false)
    let model = AppModel(client: FakeClient(), store: store, lock: makeLock())
    _ = try await model.pair(try #require(PairingLink(pairingText())), deviceName: "iPhone")
    model.reopenList()
    #expect(model.storageProblem == .notOpened)

    try setReadable(true)
    model.reopenList()

    #expect(model.hosts.map(\.hostId) == [macB, macA.hostId])
    #expect(model.storageProblem == nil)
    #expect(!model.listDidNotOpen)
    #expect(store.load().hosts.map(\.hostId) == [macB, macA.hostId])
  }
}

struct PairingLinkVersionTests {
  @Test func aCodeFromANewerVersionAsksForAnUpdate() {
    let text = pairingText().replacingOccurrences(of: "v=1", with: "v=2")
    #expect(PairingLink(text) == nil)
    #expect(PairingLink.isFromNewerVersion(text))
  }

  @Test(arguments: ["v=x", "v=0", "v=-2", "v=1"])
  func otherVersionsAreNotNewer(_ version: String) {
    #expect(!PairingLink.isFromNewerVersion(pairingText().replacingOccurrences(of: "v=1", with: version)))
  }

  @Test func onlyPairingCodesCanBeNewer() {
    #expect(!PairingLink.isFromNewerVersion("https://example.com/pair?v=2"))
  }
}

struct VersionTests {
  @Test func versionsCompareLikeTheCore() throws {
    let order = [
      "1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-alpha.beta", "1.0.0-beta", "1.0.0-beta.2", "1.0.0-beta.11",
      "1.0.0-rc.1", "1.0.0", "1.2.0-beta.1", "1.2.0", "1.3.0-beta.2", "1.3.0", "1.9.2", "1.10.0",
    ]
    let versions = try order.map { try #require(HostStore.Version($0)) }
    for (earlier, later) in zip(versions, versions.dropFirst()) {
      #expect(earlier < later, "\(earlier) < \(later)")
      #expect(!(later < earlier), "\(later) < \(earlier)")
    }
    #expect(HostStore.Version("1.2.3+build.5") == HostStore.Version("1.2.3"))
  }

  @Test(arguments: [
    "", "1.2", "1.2.3.4", "01.2.3", "1.2.3-", "1.2.3-beta..1", "1.2.3-01", "v1.2.3", "1.2.3-beta_1",
  ])
  func whatTheCoreDoesntReadIsNoVersion(_ text: String) {
    #expect(HostStore.Version(text) == nil)
  }
}

struct UpdateTextTests {
  @Test(arguments: [
    JEVisionRemote.RemoteError.updateApp(version: "7.3.1"), .updateMac(version: "7.3.1"),
  ])
  func aKnownVersionIsNamed(_ error: JEVisionRemote.RemoteError) {
    #expect(error.message.contains("7.3.1"))
  }

  @Test(arguments: [
    JEVisionRemote.RemoteError.updateApp(version: "7.3.1"), .updateApp(version: nil),
    .updateMac(version: "7.3.1"), .updateMac(version: nil),
  ])
  func sayWhatToUpdate(_ error: JEVisionRemote.RemoteError) {
    let other = JEVisionRemote.RemoteError.other
    #expect(!error.title.isEmpty && error.title != other.title)
    #expect(!error.message.isEmpty && error.message != other.message)
    #expect(error.symbol != other.symbol)
  }

  @Test func theAppAndTheMacAskForDifferentUpdates() {
    #expect(JEVisionRemote.RemoteError.updateApp(version: nil).title != JEVisionRemote.RemoteError.updateMac(version: nil).title)
    #expect(JEVisionRemote.RemoteError.updateApp(version: nil).message != JEVisionRemote.RemoteError.updateMac(version: nil).message)
  }

  /// A newer Mac may report a run state this version doesn't know: it shows as unknown and the
  /// run is still followed.
  @Test func anUnknownRunStateShowsAsUnknown() {
    let status = JEVisionMobile.RunStatus(
      run: "r", test: "a.flow.json", state: .unknown, verdict: nil, attempt: 1, block: nil, waiting: nil)
    var progress = RunProgress()
    progress.apply(.status(JEVisionRemote.RunStatus(status)))
    #expect(progress.state == .unknown)
    #expect(!progress.isFinished)
  }
}
