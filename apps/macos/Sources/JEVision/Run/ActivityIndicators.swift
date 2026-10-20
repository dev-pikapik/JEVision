import AppKit
import JEVisionHands
import Observation
import SwiftUI

/// What JEVision is doing to the Mac, as the edge glow and the menu bar item show it.
enum IndicatorState: Equatable {
  case running, paused, recording, recordingPaused

  /// `nil` when nothing runs or records. A finished run counts as over, before its window is back.
  init?(run: RunState.Phase?, isRecording: Bool, recordingPaused: Bool) {
    switch run {
    case .running: self = .running
    case .paused: self = .paused
    // A run that waits for the screen leaves the Mac to its owner.
    case .finished, .waitingForScreen, nil:
      guard isRecording else { return nil }
      self = recordingPaused ? .recordingPaused : .recording
    }
  }

  /// Paused is orange, as in the run and recording panels.
  var color: NSColor {
    switch self {
    case .running: .systemBlue
    case .paused, .recordingPaused: .systemOrange
    case .recording: .systemRed
    }
  }

  /// A pause holds still, so it reads as one at a glance.
  var pulses: Bool { self == .running || self == .recording }

  var symbol: String {
    switch self {
    case .running: "cursorarrow.click.2"
    case .paused, .recordingPaused: "pause.circle.fill"
    case .recording: "record.circle.fill"
    }
  }
}

/// Shows that JEVision drives the Mac while a test runs or records: the edges of the screen with
/// the app under test glow, and a menu bar item tells how far it got and offers the controls.
@MainActor
final class ActivityIndicators: NSObject, NSMenuDelegate {
  private let runs: RunCenter
  private let records: RecordCenter
  private var glow: NSPanel?
  private var statusItem: NSStatusItem?

  init(runs: RunCenter, records: RecordCenter) {
    self.runs = runs
    self.records = records
    super.init()
    track()
    NotificationCenter.default.addObserver(
      self, selector: #selector(screensChanged), name: NSApplication.didChangeScreenParametersNotification, object: nil)
  }

  private func track() {
    withObservationTracking { refresh() } onChange: { [weak self] in
      Task { @MainActor in self?.track() }
    }
  }

  @objc private func screensChanged() { refresh() }

  private func refresh() {
    let phase = runs.isRunning ? runs.state?.phase : nil
    // Each recorded step is a moment to see whether the app's window moved to another screen.
    _ = records.steps
    guard let activity = IndicatorState(run: phase, isRecording: records.isRecording, recordingPaused: records.isPaused),
      let screen = screen(showing: runs.isRunning ? runs.running?.document.flow?.app : records.app)
    else {
      glow?.orderOut(nil)
      glow = nil
      if let statusItem { NSStatusBar.system.removeStatusItem(statusItem) }
      statusItem = nil
      return
    }
    showGlow(activity, on: screen)
    showStatus(activity)
  }

  // MARK: Edge glow

  /// The screen with the front window of `app`; the main screen until the app has a window.
  private func screen(showing app: String?) -> NSScreen? {
    let primary = NSScreen.screens.first
    if let app, !app.isEmpty, let primary, let window = MacHands.frontWindow(of: MacHands.runningApps(app)) {
      // Window bounds count from the top of the primary screen, Cocoa from its bottom.
      let center = NSPoint(x: window.bounds.midX, y: primary.frame.maxY - window.bounds.midY)
      if let screen = NSScreen.screens.first(where: { $0.frame.contains(center) }) { return screen }
    }
    return NSScreen.main ?? primary
  }

  private func showGlow(_ activity: IndicatorState, on screen: NSScreen) {
    if let glow, let host = glow.contentView as? NSHostingView<EdgeGlow> {
      if host.rootView.activity != activity { host.rootView = EdgeGlow(activity: activity) }
      if glow.frame != screen.frame { glow.setFrame(screen.frame, display: true) }
      return
    }
    let panel = NSPanel(
      contentRect: screen.frame, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
    // Above the app under test, below the run and recording panels.
    panel.level = .floating
    panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]
    panel.ignoresMouseEvents = true
    panel.isOpaque = false
    panel.backgroundColor = .clear
    panel.hasShadow = false
    panel.hidesOnDeactivate = false
    panel.canHide = false
    panel.isReleasedWhenClosed = false
    panel.isExcludedFromWindowsMenu = true
    // Never in the run video, the screenshots a test compares, or a shared screen.
    panel.sharingType = .none
    panel.contentView = NSHostingView(rootView: EdgeGlow(activity: activity))
    panel.setFrame(screen.frame, display: false)
    panel.orderFrontRegardless()
    glow = panel
  }

  // MARK: Menu bar

  private func showStatus(_ activity: IndicatorState) {
    let item = statusItem ?? makeStatusItem()
    guard let button = item.button else { return }
    let title: String
    switch activity {
    case .running:
      let (step, total) = runs.state.map { RunHUD.step(of: $0, in: runs.running?.document) } ?? (1, 1)
      let steps = String(localized: "Step \(step) of \(total)")
      title = runs.batch.map { "\($0.progress) · \(steps)" } ?? steps
    case .paused, .recordingPaused: title = String(localized: "Paused")
    case .recording: title = String(localized: "Recording")
    }
    button.image = NSImage(systemSymbolName: activity.symbol, accessibilityDescription: nil)?
      .withSymbolConfiguration(NSImage.SymbolConfiguration(paletteColors: [activity.color]))
    button.imagePosition = .imageLeading
    button.title = title
    button.toolTip = header
    button.setAccessibilityLabel("\(header), \(title)")
  }

  private func makeStatusItem() -> NSStatusItem {
    let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
    let menu = NSMenu()
    menu.delegate = self
    item.menu = menu
    statusItem = item
    return item
  }

  private var header: String {
    runs.isRunning
      ? String(localized: "JEVision is controlling this Mac") : String(localized: "Recording in \(records.appName)")
  }

  /// Built as it opens, so it shows the state of that moment.
  func menuNeedsUpdate(_ menu: NSMenu) {
    menu.removeAllItems()
    menu.addItem(.sectionHeader(title: header))
    let paused: Bool
    if runs.isRunning, let state = runs.state {
      let (step, total) = RunHUD.step(of: state, in: runs.running?.document)
      if let batch = runs.batch {
        menu.addItem(withTitle: "\(batch.title) · \(batch.progress)", action: nil, keyEquivalent: "")
      }
      menu.addItem(withTitle: state.name.isEmpty ? String(localized: "Untitled Test") : state.name, action: nil, keyEquivalent: "")
      menu.addItem(
        withTitle: "\(RunHUD.status(of: state)) · \(String(localized: "Step \(step) of \(total)"))", action: nil,
        keyEquivalent: "")
      paused = state.isPaused
    } else {
      menu.addItem(withTitle: String(localized: "\(records.steps.count) steps"), action: nil, keyEquivalent: "")
      paused = records.isPaused
    }
    menu.addItem(.separator())
    menu.addItem(
      paused
        ? item(String(localized: "Resume"), symbol: "play.fill", action: #selector(togglePause))
        : item(String(localized: "Pause"), symbol: "pause.fill", action: #selector(togglePause)))
    if runs.isRunning {
      menu.addItem(item(String(localized: "Stop"), symbol: "stop.fill", action: #selector(stop)))
      menu.addItem(.separator())
      menu.addItem(item(String(localized: "Show JEVision"), symbol: "macwindow", action: #selector(showApp)))
    } else {
      menu.addItem(item(String(localized: "Finish Recording"), symbol: "checkmark", action: #selector(stop)))
    }
  }

  private func item(_ title: String, symbol: String, action: Selector) -> NSMenuItem {
    let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
    item.target = self
    item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)
    return item
  }

  @objc private func togglePause() {
    guard runs.isRunning else { return records.togglePause() }
    if runs.state?.isPaused == true { runs.resume() } else { runs.pause() }
  }

  /// Stops a run, or ends a recording and adds its steps to the test.
  @objc private func stop() {
    if runs.isRunning { runs.stop() } else { records.done() }
  }

  @objc private func showApp() { runs.showApp() }
}

/// Soft light along the edges of the screen.
struct EdgeGlow: View {
  let activity: IndicatorState
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  var body: some View {
    let color = Color(nsColor: activity.color)
    let glow = ZStack {
      Rectangle().strokeBorder(color.opacity(0.35), lineWidth: 40).blur(radius: 24)
      Rectangle().strokeBorder(color.opacity(0.9), lineWidth: 3)
    }
    .ignoresSafeArea()
    .accessibilityHidden(true)
    if activity.pulses, !reduceMotion {
      glow.phaseAnimator([1, 0.55]) { content, opacity in
        content.opacity(opacity)
      } animation: { _ in .easeInOut(duration: 1) }
    } else {
      glow
    }
  }
}
