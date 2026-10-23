import AppKit
import CoreImage
import CoreImage.CIFilterBuiltins
import IOKit.pwr_mgt
import JEVisionCore
import JEVisionHands
import Observation
import Synchronization

/// Remote access from paired iPhones. Lives as long as the app, so phones keep reaching the
/// Mac with Settings closed; turned on again at launch when it was on.
@MainActor
@Observable
final class RemoteAccess {
  enum Phase: Equatable {
    case off, starting, on
    case failed(String)
  }

  /// Relay servers carry the encrypted traffic when the iPhone can't reach the Mac directly.
  enum RelayChoice: String, CaseIterable, Identifiable {
    /// The public relays of iroh's maker, n0.
    case standard
    /// A relay the user runs.
    case custom
    /// Same network or VPN only.
    case none

    var id: String { rawValue }
  }

  /// What the next device to pair is.
  enum Newcomer: Equatable {
    case iPhone
    /// An AI assistant on another computer, through `jevision mcp`.
    case assistant
  }

  private(set) var phase = Phase.off
  /// Whether the code shown pairs an iPhone or an assistant on another computer.
  var newcomer = Newcomer.iPhone {
    didSet { if newcomer != oldValue, offer != nil || pendingCode { showCode() } }
  }
  /// What the next phone to pair may do.
  var nextPermission = DevicePermission.run {
    didSet { if nextPermission != oldValue, newcomer == .iPhone, offer != nil { showCode() } }
  }
  /// The pairing code shown, while Settings shows it.
  private(set) var offer: PairingCode?
  /// The code as a picture an iPhone scans; none for an assistant, who types it.
  private(set) var qrCode: CGImage?
  /// The device that paired with the code shown last.
  private(set) var justPaired: String?
  private(set) var devices: [PairedDevice] = []
  /// Ids of the devices connected now.
  private(set) var connected: Set<String> = []
  private(set) var activity: [Activity] = []
  /// Why the last change to a device didn't work.
  var failure: String?

  var isEnabled: Bool {
    didSet {
      guard isEnabled != oldValue else { return }
      defaults.set(isEnabled, forKey: Key.enabled)
      if isEnabled { turnOn() } else { turnOff() }
    }
  }
  var confirmsRuns: Bool {
    didSet {
      defaults.set(confirmsRuns, forKey: Key.confirmRuns)
      host?.setConfirmRuns(confirm: confirmsRuns)
    }
  }
  var keepsAwake: Bool {
    didSet {
      defaults.set(keepsAwake, forKey: Key.keepAwake)
      updateSleep()
    }
  }
  private(set) var relayChoice: RelayChoice
  private(set) var customRelay: String

  @ObservationIgnored private var host: RemoteHost?
  @ObservationIgnored private var sleepAssertion = IOPMAssertionID(0)
  @ObservationIgnored private let runs: RunCenter
  @ObservationIgnored private let project: ProjectStore
  @ObservationIgnored private let defaults: UserDefaults
  @ObservationIgnored private let stateDirectory: URL
  @ObservationIgnored private let secretKey: () throws -> Data

  private enum Key {
    static let enabled = "remoteEnabled"
    static let confirmRuns = "remoteConfirmRuns"
    static let keepAwake = "remoteKeepAwake"
    static let relay = "remoteRelay"
    static let customRelay = "remoteCustomRelay"
  }

  /// Entries of the activity log Settings shows.
  static let activityLimit: UInt32 = 200

  init(
    runs: RunCenter, project: ProjectStore, defaults: UserDefaults = Profile.current.defaults,
    stateDirectory: URL = Profile.current.supportFolder.appending(path: "Remote", directoryHint: .isDirectory),
    secretKey: @escaping () throws -> Data = RemoteAccess.keychainKey
  ) {
    self.runs = runs
    self.project = project
    self.defaults = defaults
    self.stateDirectory = stateDirectory
    self.secretKey = secretKey
    isEnabled = defaults.bool(forKey: Key.enabled)
    confirmsRuns = defaults.bool(forKey: Key.confirmRuns)
    keepsAwake = defaults.object(forKey: Key.keepAwake) as? Bool ?? true
    relayChoice = defaults.string(forKey: Key.relay).flatMap(RelayChoice.init) ?? .standard
    customRelay = defaults.string(forKey: Key.customRelay) ?? ""
    runs.remoteListener = { [weak self] project in self?.host?.runListener(projectRoot: project.path) }
    followProject()
    if isEnabled { turnOn() }
  }

  // MARK: On and off

  private func turnOn() {
    guard host == nil else { return }
    let relay: Relay
    switch relayChoice {
    case .standard: relay = .default
    case .custom: relay = .custom(url: customRelay.trimmingCharacters(in: .whitespaces))
    case .none: relay = .disabled
    }
    let host: RemoteHost
    do {
      host = try RemoteHost(
        config: RemoteConfig(
          secretKey: try secretKey(), name: Self.macName, stateDir: stateDirectory.path, relay: relay,
          localDiscovery: true,
          // n0's address lookup is a third party, like its relays.
          globalDiscovery: relayChoice == .standard),
        delegate: RemoteBridge(runs: runs),
        // Assistants on other computers get what the ones on this Mac get.
        assistants: MacAssistantHost(readiness: { [runs, name = Self.macName] in Self.assistantBlocker(runs: runs, mac: name) }))
    } catch {
      phase = .failed(RemoteText.error(error))
      return
    }
    self.host = host
    phase = .starting
    host.setConfirmRuns(confirm: confirmsRuns)
    host.setProjects(projects: projects)
    Task.detached {
      let started = Result { try host.start() }
      await MainActor.run { self.started(host, started) }
    }
  }

  private func started(_ started: RemoteHost, _ result: Result<Void, any Error>) {
    guard host === started else { return }
    switch result {
    case .success:
      phase = .on
      refresh()
    case .failure(let error):
      host = nil
      phase = .failed(RemoteText.error(error))
    }
    updateSleep()
  }

  private func turnOff() {
    guard let host else {
      phase = .off
      return
    }
    self.host = nil
    phase = .off
    pendingCode = false
    offer = nil
    qrCode = nil
    justPaired = nil
    connected = []
    updateSleep()
    // Closing the connections takes a moment; the main thread doesn't wait for it.
    Task.detached { host.stop() }
  }

  /// Tries again after remote access couldn't turn on.
  func retry() {
    if isEnabled { turnOn() }
  }

  /// Uses another relay from now on, reconnecting if remote access is on.
  func setRelay(_ choice: RelayChoice, customURL: String) {
    relayChoice = choice
    customRelay = customURL.trimmingCharacters(in: .whitespaces)
    defaults.set(choice.rawValue, forKey: Key.relay)
    defaults.set(customRelay, forKey: Key.customRelay)
    guard isEnabled else { return }
    let showsCode = offer != nil
    turnOff()
    turnOn()
    if showsCode { pendingCode = true }
  }

  /// Whether `text` can be a relay's address: `https://` and a host.
  static func isRelayAddress(_ text: String) -> Bool {
    guard let url = URL(string: text.trimmingCharacters(in: .whitespaces)) else { return false }
    return url.scheme == "https" && url.host()?.isEmpty == false
  }

  /// Settings asked for a code while the host was still starting.
  @ObservationIgnored private var pendingCode = false

  // MARK: Pairing

  /// Shows a new pairing code, replacing the one shown.
  func showCode() {
    justPaired = nil
    guard let host, phase == .on else {
      pendingCode = true
      return
    }
    pendingCode = false
    do {
      switch newcomer {
      case .iPhone:
        let offer = try host.createOffer(permission: nextPermission)
        self.offer = offer
        qrCode = Self.qrCode(offer.url)
      case .assistant:
        offer = try host.createAssistantOffer()
        qrCode = nil
      }
    } catch {
      offer = nil
      qrCode = nil
      failure = RemoteText.error(error)
    }
  }

  /// Settings no longer shows the code: it stops working.
  func hideCode() {
    pendingCode = false
    host?.cancelOffer()
    offer = nil
    qrCode = nil
    justPaired = nil
  }

  /// What the person types in Terminal on the other computer to pair an assistant with this Mac.
  static func pairCommand(for offer: PairingCode) -> String {
    "jevision mcp --pair \(shellWord(offer.url))"
  }

  /// What the assistant's MCP settings run on the other computer once it is paired.
  static func connectCommand(mac: String = macName) -> String {
    "jevision mcp --remote \(shellWord(mac))"
  }

  /// Whole seconds until the code shown stops working; 0 once it has.
  static func secondsLeft(of offer: PairingCode, at date: Date) -> Int {
    max(0, Int(offer.expiresAt) - Int(date.timeIntervalSince1970.rounded(.up)))
  }

  /// Reads devices, connections and the log again. Settings calls it while it is open.
  func refresh() {
    guard let host, phase == .on else { return }
    if pendingCode { showCode() }
    let devices = host.devices()
    if let offer, let paired = Self.newlyPaired(devices, before: self.devices, offer: offer) {
      // The code works once: the next phone needs a new one.
      host.cancelOffer()
      self.offer = nil
      qrCode = nil
      justPaired = paired.name
    }
    self.devices = devices
    connected = Set(host.status().connected)
    activity = host.activity(limit: Self.activityLimit)
  }

  /// The device that paired with `offer` since `before` was read.
  static func newlyPaired(_ devices: [PairedDevice], before: [PairedDevice], offer: PairingCode) -> PairedDevice? {
    let known = Set(before.map(\.id))
    // A code works five minutes, so it was made at most that long before it expires.
    return devices.last { !known.contains($0.id) && $0.pairedAt >= offer.expiresAt - 5 * 60 }
  }

  // MARK: Devices

  func setPermission(_ permission: DevicePermission, of device: PairedDevice) {
    perform { try $0.setPermission(deviceId: device.id, permission: permission) }
  }

  /// Lets an assistant use the tools of this Mac or not. Turning it off ends what it started.
  func setAssistant(_ allowed: Bool, of device: PairedDevice) {
    perform { try $0.setAssistantEnabled(deviceId: device.id, enabled: allowed) }
  }

  func revoke(_ device: PairedDevice) {
    perform { try $0.revoke(deviceId: device.id) }
  }

  /// Changes a device off the main thread: disconnecting an assistant waits for its run to stop.
  private func perform(_ change: @escaping @Sendable (RemoteHost) throws -> Void) {
    guard let host else { return }
    Task.detached {
      let result = Result { try change(host) }
      await MainActor.run {
        if case .failure(let error) = result { self.failure = RemoteText.error(error) }
        self.refresh()
      }
    }
  }

  // MARK: Projects

  private var projects: [RemoteProject] {
    guard let root = project.rootURL else { return [] }
    let name = project.name.isEmpty ? root.lastPathComponent : project.name
    return [RemoteProject(name: name, root: root.path)]
  }

  /// Phones see the project open in the app, whichever it is now.
  private func followProject() {
    withObservationTracking {
      _ = projects
    } onChange: { [weak self] in
      Task { @MainActor in
        guard let self else { return }
        self.host?.setProjects(projects: self.projects)
        self.followProject()
      }
    }
  }

  // MARK: Sleep

  /// While remote access is on, the Mac doesn't fall asleep on its own; the display still may.
  private func updateSleep() {
    let wanted = phase == .on && keepsAwake
    if wanted, sleepAssertion == 0 {
      let reason = String(localized: "Remote access is on: paired iPhones can run tests on this Mac.")
      if IOPMAssertionCreateWithName(
        kIOPMAssertionTypePreventUserIdleSystemSleep as CFString, IOPMAssertionLevel(kIOPMAssertionLevelOn),
        reason as CFString, &sleepAssertion) != kIOReturnSuccess
      {
        sleepAssertion = 0
      }
    } else if !wanted, sleepAssertion != 0 {
      IOPMAssertionRelease(sleepAssertion)
      sleepAssertion = 0
    }
  }

  var keepsMacAwakeNow: Bool { sleepAssertion != 0 }

  // MARK: Helpers

  /// The Mac's name as phones show it, as set in System Settings → General → Sharing.
  static var macName: String {
    Host.current().localizedName ?? String(localized: "Mac")
  }

  /// What keeps an assistant on another computer from running a test now, in words for it to
  /// pass on; `nil` when nothing does. What a person at the terminal of this Mac would be told too.
  nonisolated static func assistantBlocker(runs: RunCenter, mac: String) -> String? {
    if MacHands.screenIsLocked {
      return "The screen of “\(mac)” is locked, so tests can’t run there. Ask the person at that Mac to unlock it, then try again."
    }
    let missing = [
      CGPreflightScreenCaptureAccess() ? nil : "record the screen",
      AXIsProcessTrusted() ? nil : "control the computer",
    ].compactMap(\.self)
    if !missing.isEmpty {
      return "JEVision on “\(mac)” isn’t allowed to \(missing.joined(separator: " and ")) yet. The person at that Mac can allow it in JEVision → Settings → Permissions."
    }
    let busy = DispatchQueue.main.sync { MainActor.assumeIsolated { runs.isRunning || runs.blocked != nil } }
    return busy ? "A test is already running in JEVision on “\(mac)”. Try again when it has ended." : nil
  }

  nonisolated private static let keyService = "JEVision Remote"
  nonisolated private static let keyName = "host-key"

  /// The Mac's key for remote access from the Keychain, made the first time. Phones know the
  /// Mac by it, so it stays the same between launches.
  nonisolated static func keychainKey() throws -> Data {
    if let saved = SecretStore.value(keyName, in: keyService), let key = Data(base64Encoded: saved), key.count == 32 {
      return key
    }
    let key = generateRemoteKey()
    try SecretStore.save(key.base64EncodedString(), for: keyName, in: keyService)
    return key
  }

  /// A QR code of `text`, one pixel per module; draw it without smoothing.
  static func qrCode(_ text: String) -> CGImage? {
    let filter = CIFilter.qrCodeGenerator()
    filter.message = Data(text.utf8)
    filter.correctionLevel = "M"
    guard let image = filter.outputImage else { return nil }
    return CIContext().createCGImage(image, from: image.extent)
  }
}

/// The app's side of remote access, called on the host's background threads.
private final class RemoteBridge: RemoteDelegate {
  let runs: RunCenter

  init(runs: RunCenter) {
    self.runs = runs
  }

  func runBlocker() -> RemoteError? {
    if MacHands.screenIsLocked { return .MacLocked }
    let busy = DispatchQueue.main.sync { MainActor.assumeIsolated { runs.isRunning || runs.blocked != nil } }
    return busy ? .RunInProgress : nil
  }

  func confirmRun(device: PairedDevice, project: String, test: String) -> Bool {
    let allowed = Mutex(false)
    let answered = DispatchSemaphore(value: 0)
    // A run loop block, not a main queue one: the main queue keeps working while the alert waits.
    RunLoop.main.perform(inModes: [.common]) {
      MainActor.assumeIsolated {
        let answer = RemoteConfirmation.ask(device: device.name, project: project, test: test)
        allowed.withLock { $0 = answer }
      }
      answered.signal()
    }
    answered.wait()
    return allowed.withLock { $0 }
  }

  func startRun(projectRoot: String, test: String, dataRow: UInt32?, requestedBy: PairedDevice) -> RemoteError? {
    DispatchQueue.main.sync {
      MainActor.assumeIsolated {
        runs.runRemotely(
          test, in: URL(filePath: projectRoot, directoryHint: .isDirectory), dataRow: dataRow.map(Int.init),
          requestedBy: requestedBy.name)
      }
    }
  }

  func stopRun() {
    DispatchQueue.main.async { [runs] in
      MainActor.assumeIsolated { runs.stop() }
    }
  }
}

/// Asks the person at the Mac before a phone's run.
@MainActor
enum RemoteConfirmation {
  /// Time the question waits for an answer before the run is declined.
  static let timeout: TimeInterval = 120

  static func ask(device: String, project: String, test: String) -> Bool {
    let alert = NSAlert()
    alert.messageText = String(localized: "Allow “\(device)” to run a test?")
    alert.informativeText = String(
      localized: "\(test) in “\(project)”. The test will use the mouse and keyboard of this Mac until it ends.")
    alert.addButton(withTitle: String(localized: "Allow"))
    alert.addButton(withTitle: String(localized: "Don’t Allow"))
    let timer = Timer(timeInterval: timeout, repeats: false) { _ in
      MainActor.assumeIsolated { NSApp.abortModal() }
    }
    RunLoop.main.add(timer, forMode: .modalPanel)
    defer { timer.invalidate() }
    NSApp.activate()
    return alert.runModal() == .alertFirstButtonReturn
  }
}
