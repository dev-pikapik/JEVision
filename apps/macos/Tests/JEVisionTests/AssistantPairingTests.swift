import CryptoKit
import Foundation
import JEVisionCore
import JEVisionHands
import Testing

@testable import JEVisionCLI

struct AssistantPairingTests {
  @Test func theComputerKeySignsWhatTheMacChecks() throws {
    let key = ComputerKey(P256.Signing.PrivateKey())
    let public65 = key.publicKey()
    #expect(
      public65.count == 65 && public65.first == 0x04, "SEC1 uncompressed, as the Mac reads it")
    let message = Data("challenge".utf8)
    let signature = try #require(key.sign(message: message))
    let valid = try P256.Signing.PublicKey(x963Representation: public65)
      .isValidSignature(P256.Signing.ECDSASignature(derRepresentation: signature), for: message)
    #expect(valid, "DER over the SHA-256 of the message")
    #expect(
      try !P256.Signing.PublicKey(x963Representation: public65)
        .isValidSignature(
          P256.Signing.ECDSASignature(derRepresentation: signature), for: Data("other".utf8)))
  }

  private func macs() -> RemoteMacs {
    RemoteMacs(
      file: FileManager.default.temporaryDirectory.appending(
        path: "jevision-macs-\(UUID().uuidString)/Assistant/macs.json"))
  }

  @Test func pairedMacsAreKeptAndFoundByNameOrId() throws {
    let store = macs()
    defer {
      try? FileManager.default.removeItem(
        at: store.file.deletingLastPathComponent().deletingLastPathComponent())
    }
    #expect(try store.all().isEmpty, "no file yet is no Mac")
    let studio = HostRecord(
      hostId: "ab12cd34ef56", name: "Studio", deviceId: "d1", relayUrl: "https://relay.example.com",
      addrs: ["10.0.0.2:1"])
    let office = HostRecord(
      hostId: "cd34ef56ab12", name: "Anna’s Mac", deviceId: "d2", relayUrl: nil, addrs: [])
    try store.save(studio)
    try store.save(office)
    try store.save(
      HostRecord(
        hostId: studio.hostId, name: "Studio", deviceId: "d1", relayUrl: nil, addrs: ["10.0.0.3:1"])
    )
    let all = try store.all()
    #expect(
      all.map(\.hostId) == [office.hostId, studio.hostId],
      "pairing again replaces the Mac, it isn't listed twice")
    #expect(all.last?.addrs == ["10.0.0.3:1"])

    #expect(
      try store.matching("studio").map(\.hostId) == [studio.hostId], "the case doesn't matter")
    #expect(try store.matching("anna’s mac").map(\.hostId) == [office.hostId])
    #expect(
      try store.matching("cd34ef").map(\.hostId) == [office.hostId], "nor does the start of the id")
    #expect(try store.matching("cd34").isEmpty, "a short start of an id is too vague")
    #expect(try store.matching("Nobody").isEmpty)

    try store.remove(studio)
    #expect(try store.all().map(\.name) == ["Anna’s Mac"])
    let mode =
      try FileManager.default.attributesOfItem(atPath: store.file.path)[.posixPermissions] as? Int
    #expect(mode == 0o600, "only this user reads where the Macs are")
  }

  @Test func aDamagedFileIsAnErrorNotAnEmptyList() throws {
    let store = macs()
    defer {
      try? FileManager.default.removeItem(
        at: store.file.deletingLastPathComponent().deletingLastPathComponent())
    }
    try FileManager.default.createDirectory(
      at: store.file.deletingLastPathComponent(), withIntermediateDirectories: true)
    try Data("not json".utf8).write(to: store.file)
    #expect(throws: (any Error).self) { try store.all() }
  }

  @Test func legacyMacsMigrateOnceWithAnUntouchedBackup() throws {
    let store = macs()
    defer { try? FileManager.default.removeItem(at: store.file.deletingLastPathComponent()) }
    try FileManager.default.createDirectory(
      at: store.file.deletingLastPathComponent(), withIntermediateDirectories: true)
    let legacy = Data(
      #"[{"hostId":"old","name":"Old Mac","deviceId":"d1","addrs":[],"future":"keep"}]"#.utf8)
    try legacy.write(to: store.file)
    #expect(try store.all().map(\.name) == ["Old Mac"])
    #expect(try Data(contentsOf: store.file) == legacy, "reading never migrates")
    let updated = HostRecord(
      hostId: "old", name: "Renamed", deviceId: "d1", relayUrl: nil, addrs: [])
    try store.save(updated)
    let backup = store.file.deletingPathExtension().appendingPathExtension("v0.json")
    #expect(try Data(contentsOf: backup) == legacy)
    let json = try #require(
      JSONSerialization.jsonObject(with: Data(contentsOf: store.file)) as? [String: Any])
    #expect(json["format"] as? Int == 1)
    #expect((json["macs"] as? [[String: Any]])?.first?["future"] as? String == "keep")
    try store.save(updated)
    #expect(try Data(contentsOf: backup) == legacy)
  }

  @Test func savingKeepsUnknownFieldsAndUnreadableEntries() throws {
    let store = macs()
    defer { try? FileManager.default.removeItem(at: store.file.deletingLastPathComponent()) }
    try FileManager.default.createDirectory(
      at: store.file.deletingLastPathComponent(), withIntermediateDirectories: true)
    try Data(
      #"{"format":1,"extra":{"x":1},"macs":[{"hostId":"one","name":"Mac","deviceId":"d","relayUrl":"old","addrs":[],"feature":"kept"},{"futureEntry":true}]}"#
        .utf8
    ).write(to: store.file)
    try store.save(
      HostRecord(hostId: "one", name: "Changed", deviceId: "d", relayUrl: nil, addrs: ["new"]))
    let json = try #require(
      JSONSerialization.jsonObject(with: Data(contentsOf: store.file)) as? [String: Any])
    #expect((json["extra"] as? [String: Int])?["x"] == 1)
    let entries = try #require(json["macs"] as? [[String: Any]])
    #expect(entries.count == 2)
    #expect(entries.first?["futureEntry"] as? Bool == true)
    #expect(entries.last?["feature"] as? String == "kept")
    #expect(entries.last?["relayUrl"] == nil)
    try store.remove(try #require(store.all().first))
    let remaining = try #require(
      JSONSerialization.jsonObject(with: Data(contentsOf: store.file)) as? [String: Any])
    #expect((remaining["macs"] as? [[String: Bool]]) == [["futureEntry": true]])
  }

  @Test func newerAndMalformedListsCannotBeOverwritten() throws {
    let store = macs()
    defer { try? FileManager.default.removeItem(at: store.file.deletingLastPathComponent()) }
    try FileManager.default.createDirectory(
      at: store.file.deletingLastPathComponent(), withIntermediateDirectories: true)
    let mac = HostRecord(hostId: "one", name: "Mac", deviceId: "d", relayUrl: nil, addrs: [])
    for text in [
      #"{"format":2,"macs":[],"future":true}"#,
      #"{"format":1,"minVersion":"99.0.0","macs":[]}"#,
      #"{"format":1,"minVersion":"invalid","macs":[]}"#,
      #"{"format":true,"macs":[]}"#,
      #"{"format":1.5,"macs":[]}"#,
      #"{"format":1,"other":[]}"#, "broken JSON",
    ] {
      let original = Data(text.utf8)
      try original.write(to: store.file)
      #expect(throws: (any Error).self) { try store.save(mac) }
      #expect(throws: (any Error).self) { try store.remove(mac) }
      #expect(try Data(contentsOf: store.file) == original)
    }
  }

  @Test func storedVersionsUseSemanticVersionOrdering() throws {
    let version = { try #require(RemoteMacs.Version($0)) }
    #expect(try version("1.0.0-beta.2") < version("1.0.0-beta.10"))
    #expect(try version("1.0.0-beta.10") < version("1.0.0"))
    #expect(try version("1.2.0+build") < version("1.10.0"))
    #expect(RemoteMacs.Version("01.0.0") == nil)
  }

  @Test func theHostGivesTheMacsFacesToTheServer() throws {
    let host = MacAssistantHost(readiness: { "The screen is locked." })
    #expect(host.readinessProblem() == "The screen is locked.")
    #expect(host.sharedFolder() == AISettings.assistantsFolder.path)
    #expect(throws: HandsError.self, "a run the Mac isn't ready for doesn't start") {
      try host.startRun(run: TestRun())
    }
    #expect(MacAssistantHost(readiness: { nil }).readinessProblem() == nil)
  }

  @Test func wordsAreQuotedForTheShell() {
    #expect(shellWord("Studio") == "Studio")
    #expect(shellWord("Anna’s Mac") == "'Anna’s Mac'")
    #expect(shellWord("it's") == "'it'\\''s'")
    #expect(shellWord("jevision://assistant?a=1&b=2") == "'jevision://assistant?a=1&b=2'")
    #expect(shellWord("") == "''")
  }
}
