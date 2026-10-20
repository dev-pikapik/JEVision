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
    #expect(public65.count == 65 && public65.first == 0x04, "SEC1 uncompressed, as the Mac reads it")
    let message = Data("challenge".utf8)
    let signature = try #require(key.sign(message: message))
    let valid = try P256.Signing.PublicKey(x963Representation: public65)
      .isValidSignature(P256.Signing.ECDSASignature(derRepresentation: signature), for: message)
    #expect(valid, "DER over the SHA-256 of the message")
    #expect(try !P256.Signing.PublicKey(x963Representation: public65)
      .isValidSignature(P256.Signing.ECDSASignature(derRepresentation: signature), for: Data("other".utf8)))
  }

  private func macs() -> RemoteMacs {
    RemoteMacs(
      file: FileManager.default.temporaryDirectory.appending(path: "jevision-macs-\(UUID().uuidString)/Assistant/macs.json"))
  }

  @Test func pairedMacsAreKeptAndFoundByNameOrId() throws {
    let store = macs()
    defer { try? FileManager.default.removeItem(at: store.file.deletingLastPathComponent().deletingLastPathComponent()) }
    #expect(try store.all().isEmpty, "no file yet is no Mac")
    let studio = HostRecord(hostId: "ab12cd34ef56", name: "Studio", deviceId: "d1", relayUrl: "https://relay.example.com", addrs: ["10.0.0.2:1"])
    let office = HostRecord(hostId: "cd34ef56ab12", name: "Anna’s Mac", deviceId: "d2", relayUrl: nil, addrs: [])
    try store.save(studio)
    try store.save(office)
    try store.save(HostRecord(hostId: studio.hostId, name: "Studio", deviceId: "d1", relayUrl: nil, addrs: ["10.0.0.3:1"]))
    let all = try store.all()
    #expect(all.map(\.hostId) == [office.hostId, studio.hostId], "pairing again replaces the Mac, it isn't listed twice")
    #expect(all.last?.addrs == ["10.0.0.3:1"])

    #expect(try store.matching("studio").map(\.hostId) == [studio.hostId], "the case doesn't matter")
    #expect(try store.matching("anna’s mac").map(\.hostId) == [office.hostId])
    #expect(try store.matching("cd34ef").map(\.hostId) == [office.hostId], "nor does the start of the id")
    #expect(try store.matching("cd34").isEmpty, "a short start of an id is too vague")
    #expect(try store.matching("Nobody").isEmpty)

    try store.remove(studio)
    #expect(try store.all().map(\.name) == ["Anna’s Mac"])
    let mode = try FileManager.default.attributesOfItem(atPath: store.file.path)[.posixPermissions] as? Int
    #expect(mode == 0o600, "only this user reads where the Macs are")
  }

  @Test func aDamagedFileIsAnErrorNotAnEmptyList() throws {
    let store = macs()
    defer { try? FileManager.default.removeItem(at: store.file.deletingLastPathComponent().deletingLastPathComponent()) }
    try FileManager.default.createDirectory(at: store.file.deletingLastPathComponent(), withIntermediateDirectories: true)
    try Data("not json".utf8).write(to: store.file)
    #expect(throws: (any Error).self) { try store.all() }
  }

  @Test func theHostGivesTheMacsFacesToTheServer() throws {
    let host = MacAssistantHost(readiness: { "The screen is locked." })
    #expect(host.readinessProblem() == "The screen is locked.")
    #expect(host.sharedFolder() == AISettings.assistantsFolder.path)
    #expect(throws: HandsError.self, "a run the Mac isn't ready for doesn't start") { try host.startRun(run: TestRun()) }
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
