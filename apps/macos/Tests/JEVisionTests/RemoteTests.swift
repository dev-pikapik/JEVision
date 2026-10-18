import Foundation
import JEVisionCore
import Testing

@testable import JEVision

@MainActor
struct RemoteAccessTests {
  @Test func turnsOnShowsACodeAndRemembersTheChoices() async throws {
    let suite = "jevision-tests-\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    let folder = FileManager.default.temporaryDirectory.appending(path: "jevision-remote-\(UUID().uuidString)")
    defer {
      defaults.removePersistentDomain(forName: suite)
      try? FileManager.default.removeItem(at: folder)
    }
    let key = generateRemoteKey()
    let make = {
      RemoteAccess(
        runs: RunCenter(defaults: defaults), project: ProjectStore(), defaults: defaults, stateDirectory: folder,
        secretKey: { key })
    }

    let remote = make()
    #expect(remote.phase == .off)
    #expect(!remote.isEnabled && !remote.confirmsRuns && remote.keepsAwake)
    #expect(remote.relayChoice == .standard)

    // No relay: the test stays on this Mac.
    remote.setRelay(.none, customURL: "")
    remote.confirmsRuns = true
    remote.isEnabled = true
    remote.showCode()
    for _ in 0..<100 where remote.phase == .starting {
      try await Task.sleep(for: .milliseconds(100))
    }
    #expect(remote.phase == .on)
    #expect(remote.keepsMacAwakeNow)

    // The code Settings asked for while starting appears once the host is on.
    remote.refresh()
    let offer = try #require(remote.offer)
    #expect(offer.url.hasPrefix("jevision://"))
    #expect((295...300).contains(RemoteAccess.secondsLeft(of: offer, at: .now)))
    #expect(RemoteAccess.secondsLeft(of: offer, at: .now.addingTimeInterval(400)) == 0)
    let qr = try #require(remote.qrCode)
    #expect(qr.width == qr.height && qr.width > 21)
    #expect(remote.devices.isEmpty)

    remote.hideCode()
    #expect(remote.offer == nil && remote.qrCode == nil)
    remote.isEnabled = false
    #expect(remote.phase == .off)
    #expect(!remote.keepsMacAwakeNow)

    let again = make()
    #expect(!again.isEnabled && again.confirmsRuns && again.relayChoice == .none)
  }

  @Test func tellsWhichDevicePairedWithTheCode() {
    let offer = PairingCode(url: "jevision://pair", expiresAt: 10_000)
    let old = PairedDevice(id: "a", name: "Old iPad", permission: .view, pairedAt: 1_000, lastSeen: nil)
    let new = PairedDevice(id: "b", name: "Anna’s iPhone", permission: .run, pairedAt: 9_800, lastSeen: 9_800)
    #expect(RemoteAccess.newlyPaired([old], before: [old], offer: offer) == nil)
    #expect(RemoteAccess.newlyPaired([old, new], before: [old], offer: offer) == new)
    // Paired before this code existed: another code did it.
    #expect(RemoteAccess.newlyPaired([old], before: [], offer: offer) == nil)
  }

  @Test func acceptsOnlySecureRelayAddresses() {
    #expect(RemoteAccess.isRelayAddress("https://relay.example.com"))
    #expect(RemoteAccess.isRelayAddress(" https://relay.example.com:8443/ "))
    #expect(!RemoteAccess.isRelayAddress("http://relay.example.com"))
    #expect(!RemoteAccess.isRelayAddress("relay.example.com"))
    #expect(!RemoteAccess.isRelayAddress(""))
  }

  @Test func explainsErrors() {
    #expect(RemoteText.text(.MacLocked) == "The Mac’s screen is locked.")
    #expect(RemoteText.kind(.runStarted) == "Started a run")
  }
}
