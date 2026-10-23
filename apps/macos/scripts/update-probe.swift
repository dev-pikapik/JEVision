// The update test bench's look at a feed, the way an installed JEVision sees it: the update Sparkle
// would offer, its release notes in the language of this process and its data format, or why there
// is none. update-e2e.sh builds it into UpdateProbe.app, as Sparkle picks the language of the notes
// from the languages of the app that runs it.
//   UpdateProbe.app/Contents/MacOS/update-probe FEED_URL JEVision.app [beta] [-AppleLanguages "(ru)"]
// The version, the build number, the update key and the rest of the Info.plist of JEVision.app go
// into a copy with an identifier of its own, so the settings of an installed JEVision stay untouched.
// "beta" also allows the beta channel. PROBE_BUILD=1 stands in for the build number of the app, and
// PROBE_OTHER_KEY=1 puts a new update key of its own into the copy, as if the feed were signed by
// someone else.
// Prints one line: "update <version> <build> format=<data format> notes=<first words>", "none <why>"
// ("up-to-date", or "newer-than-the-feed" when the feed has only older versions)
// or "refused <error>".
import CryptoKit
import Foundation
import Sparkle

func fail(_ message: String) -> Never {
  FileHandle.standardError.write(Data((message + "\n").utf8))
  exit(2)
}

let arguments = Array(CommandLine.arguments.dropFirst().prefix { !$0.hasPrefix("-") })
guard arguments.count == 2 || (arguments.count == 3 && arguments[2] == "beta") else {
  fail("usage: update-probe FEED_URL JEVision.app [beta]")
}
let plist = URL(fileURLWithPath: arguments[1]).appending(path: "Contents/Info.plist")
guard let data = try? Data(contentsOf: plist),
  var info = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any]
else { fail("Can't read \(plist.path)") }
let id = "io.github.dev-pikapik.jevision.update-probe.\(UUID().uuidString)"
info["CFBundleIdentifier"] = id
info["SUEnableAutomaticChecks"] = false
let environment = ProcessInfo.processInfo.environment
if let build = environment["PROBE_BUILD"] { info["CFBundleVersion"] = build }
if environment["PROBE_OTHER_KEY"] != nil {
  info["SUPublicEDKey"] = Curve25519.Signing.PrivateKey().publicKey.rawRepresentation.base64EncodedString()
}
let host = FileManager.default.temporaryDirectory.appending(path: "\(id).app")
do {
  try FileManager.default.createDirectory(at: host.appending(path: "Contents"), withIntermediateDirectories: true)
  try PropertyListSerialization.data(fromPropertyList: info, format: .xml, options: 0)
    .write(to: host.appending(path: "Contents/Info.plist"))
} catch {
  fail("Can't write the copy of the app: \(error.localizedDescription)")
}
guard let bundle = Bundle(url: host) else { fail("Can't open \(host.path)") }

final class Probe: NSObject, SPUUpdaterDelegate {
  let feed: String
  let channels: Set<String>
  var outcome: String?

  init(feed: String, channels: Set<String>) {
    self.feed = feed
    self.channels = channels
  }

  func feedURLString(for updater: SPUUpdater) -> String? { feed }
  func allowedChannels(for updater: SPUUpdater) -> Set<String> { channels }

  func updater(_ updater: SPUUpdater, didFindValidUpdate item: SUAppcastItem) {
    let notes = (item.itemDescription ?? "").split(whereSeparator: \.isNewline).first.map { String($0.prefix(40)) } ?? ""
    // Read the way Updates.swift reads it.
    let format = item.propertiesDictionary["jevision:dataFormat"] as? String ?? "-"
    outcome = outcome ?? "update \(item.displayVersionString) \(item.versionString) format=\(format) notes=\(notes)"
  }

  func updaterDidNotFindUpdate(_ updater: SPUUpdater, error: any Error) {
    let number = (error as NSError).userInfo[SPUNoUpdateFoundReasonKey] as? NSNumber
    let why =
      switch number.flatMap({ SPUNoUpdateFoundReason(rawValue: $0.int32Value) }) {
      case .onLatestVersion: "up-to-date"
      case .onNewerThanLatestVersion: "newer-than-the-feed"
      default: "reason=\(number?.stringValue ?? "-")"
      }
    outcome = outcome ?? "none \(why)"
  }

  func updater(_ updater: SPUUpdater, didAbortWithError error: any Error) {
    outcome = outcome ?? "refused \((error as NSError).code) \(error.localizedDescription)"
  }

  func updater(_ updater: SPUUpdater, didFinishUpdateCycleFor updateCheck: SPUUpdateCheck, error: (any Error)?) {
    print(outcome ?? error.map { "refused \(($0 as NSError).code) \($0.localizedDescription)" } ?? "none")
    UserDefaults.standard.removePersistentDomain(forName: id)
    try? FileManager.default.removeItem(at: host)
    exit(0)
  }
}

MainActor.assumeIsolated {
  let probe = Probe(feed: arguments[0], channels: arguments.count == 3 ? [arguments[2]] : [])
  let updater = SPUUpdater(
    hostBundle: bundle, applicationBundle: bundle, userDriver: SPUStandardUserDriver(hostBundle: bundle, delegate: nil),
    delegate: probe)
  do {
    try updater.start()
  } catch {
    fail("Sparkle didn't start: \(error.localizedDescription)")
  }
  updater.checkForUpdateInformation()
  withExtendedLifetime((probe, updater)) { RunLoop.main.run() }
}
