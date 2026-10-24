import AppKit
import JEVisionCore
import JEVisionHands
import Observation
import SwiftUI

/// Records a test from what a person does: checks that recording can start, opens the app, gets
/// the window out of the way, shows the recording panel, and puts the recorded blocks into the
/// test when the person is done.
@MainActor
@Observable
final class RecordCenter {
  struct Request: Identifiable {
    let id = UUID()
    let document: FlowDocument
    let project: URL
    /// Where the blocks go: after this block, or at the end of the test.
    let slot: Slot?
  }

  /// The recording that waits for a permission or an app; shown as a sheet.
  var blocked: Request?
  /// The recording under way.
  private(set) var recording: Request?
  private(set) var steps: [RecordedStep] = []
  private(set) var notice: Recorder.Notice?
  private(set) var isPaused = false
  /// Secrets the recording typed that have no value yet, to ask for one by one.
  var unsetSecrets: [String] = []
  private(set) var secretsProject: URL?
  /// Why the recording couldn't be added to the test.
  var failure: String?

  @ObservationIgnored private var recorder: Recorder?
  @ObservationIgnored private(set) var app = ""
  @ObservationIgnored private var panel: GlassPanel?
  @ObservationIgnored private var areaPicker: AreaPicker?
  @ObservationIgnored private var noticeTask: Task<Void, Never>?

  var isRecording: Bool { recording != nil }

  var appName: String { AppCatalog.shared.name(of: app) }

  // MARK: Starting

  /// The app the test is about, if one is chosen.
  static func app(of request: Request) -> String? {
    request.document.flow.flatMap { $0.app.isEmpty ? nil : $0.app }
  }

  static func missingPermissions() -> [Permission] {
    Permission.allCases.filter { !$0.isGranted }
  }

  /// Records into `document` after its selected block, or asks for what's missing first.
  func record(_ document: FlowDocument, in project: URL?) {
    guard let project, !isRecording, document.flow != nil, !document.isReadOnly else { return }
    let request = Request(document: document, project: project, slot: document.slotAfterSelection)
    if let app = Self.app(of: request), AppCatalog.shared.isInstalled(app), Self.missingPermissions().isEmpty {
      start(request, app: app)
    } else {
      blocked = request
    }
  }

  /// Starts the recording the sheet was holding back, in `app`.
  func confirm(app: String) {
    guard let request = blocked else { return }
    blocked = nil
    start(request, app: app)
  }

  private func start(_ request: Request, app: String) {
    let flow = request.document.flow
    let opens = !(flow?.blocks.contains { ["openApp", "openFile"].contains($0.kind.typeName) } ?? false)
    let recorder = Recorder(app: app, appName: AppCatalog.shared.name(of: app), opensApp: opens)
    // Called on the recorder's threads.
    let started = recorder.start { [weak self] update in
      DispatchQueue.main.async { MainActor.assumeIsolated { self?.apply(update) } }
    } onStop: { [weak self] in
      DispatchQueue.main.async { MainActor.assumeIsolated { self?.done() } }
    }
    guard started else {
      blocked = request
      return
    }
    self.recorder = recorder
    self.app = app
    recording = request
    steps = opens ? [.openApp] : []
    notice = nil
    isPaused = false
    failure = nil
    if let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: app) {
      let configuration = NSWorkspace.OpenConfiguration()
      configuration.activates = true
      NSWorkspace.shared.openApplication(at: url, configuration: configuration)
    }
    // Hidden rather than windowless, as for a run: see `RunCenter.getOutOfTheWay`.
    NSApp.hide(nil)
    let panel = GlassPanel(RecordHUD(center: self), width: RecordHUD.width)
    self.panel = panel
    panel.show()
    // Off the app's window once the recorder has seen it.
    Task { [weak panel] in
      try? await Task.sleep(for: .seconds(1))
      if let window = recorder.windowBounds { panel?.avoid(window) }
    }
    AccessibilityNotification.Announcement(String(localized: "Recording. Use the app; JEVision turns what you do into steps.")).post()
  }

  // MARK: Control

  func togglePause() {
    isPaused.toggle()
    recorder?.setIgnoringInput(isPaused)
  }

  func undoLastStep() {
    recorder?.undoLastStep()
  }

  func addWait() {
    recorder?.addWait()
  }

  /// Lets the person drag a frame around the part of the screen to check.
  func addCheck() {
    guard let recorder, areaPicker == nil else { return }
    recorder.setIgnoringInput(true)
    areaPicker = AreaPicker { [weak self] area in
      guard let self else { return }
      areaPicker = nil
      recorder.setIgnoringInput(isPaused)
      if let area { recorder.addCheck(area: area) }
    }
  }

  /// Ends the recording and adds its blocks to the test.
  func done() {
    guard let recorder, let request = recording else { return }
    self.recorder = nil
    areaPicker?.cancel()
    let app = app
    recorder.finish { recording in
      DispatchQueue.main.async {
        MainActor.assumeIsolated { [weak self] in self?.finish(recording, request: request, app: app) }
      }
    }
  }

  private func finish(_ result: Recording, request: Request, app: String) {
    recording = nil
    steps = []
    notice = nil
    panel?.close()
    panel = nil
    NSApp.unhideWithoutActivation()
    let configuration = NSWorkspace.OpenConfiguration()
    configuration.activates = true
    NSWorkspace.shared.openApplication(at: Bundle.main.bundleURL, configuration: configuration)
    guard !result.isEmpty else { return }
    do {
      let folder = request.document.url.deletingLastPathComponent()
      let (blocks, files) = try Recording.blocks(for: result.steps, testFolder: folder)
      request.document.insertRecorded(blocks, files: files, at: request.slot, app: app)
      secretsProject = request.project
      unsetSecrets = result.secrets.filter { !ProjectSecrets(project: request.project).isSet($0, environment: nil) }
      let count = blocks.count
      AccessibilityNotification.Announcement(String(localized: "Added \(count) steps to the test.")).post()
    } catch {
      failure = String(localized: "The recording couldn’t be added to the test: \(error.localizedDescription)")
    }
  }

  // MARK: Updates

  private func apply(_ update: Recorder.Update) {
    guard isRecording else { return }
    let added = update.steps.count > steps.count
    steps = update.steps
    if added, let last = steps.last {
      AccessibilityNotification.Announcement(RecordHUD.text(of: last, appName: appName)).post()
    }
    if let notice = update.notice {
      self.notice = notice
      AccessibilityNotification.Announcement(RecordHUD.text(of: notice)).post()
      noticeTask?.cancel()
      noticeTask = Task { [weak self] in
        try? await Task.sleep(for: .seconds(6))
        guard !Task.isCancelled else { return }
        self?.notice = nil
      }
    } else if added {
      notice = nil
    }
  }
}
