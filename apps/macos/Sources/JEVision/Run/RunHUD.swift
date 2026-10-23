import AppKit
import JEVisionCore
import JEVisionHands
import SwiftUI

/// A floating glass panel shown while the window is away: during a run or a recording. It stays
/// above every app, never takes the focus from the app under test and keeps to a corner of the
/// screen that window doesn't cover. Never being the key window, it would draw its texts and
/// controls as those of a window in the background, so its content is given the look of one in
/// front; the buttons that carry a colour use `HUDButtonStyle`, as glass drops its tint here.
@MainActor
final class GlassPanel {
  private let panel: NSPanel
  private let size: NSSize

  /// `height` defaults to what `content` needs.
  init(_ content: some View, width: Double, height: Double? = nil) {
    let view = FirstMouseHostingView(
      rootView: content.environment(\.controlActiveState, .key).defaultAppStorage(Profile.current.defaults))
    size = NSSize(width: width, height: height ?? view.fittingSize.height)
    panel = HUDPanel(
      contentRect: NSRect(origin: .zero, size: size), styleMask: [.borderless, .nonactivatingPanel], backing: .buffered,
      defer: true)
    panel.level = .statusBar
    panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .ignoresCycle]
    panel.isOpaque = false
    panel.backgroundColor = .clear
    panel.hasShadow = false
    panel.hidesOnDeactivate = false
    // Stays while JEVision hides for a run or a recording.
    panel.canHide = false
    panel.isReleasedWhenClosed = false
    panel.contentView = view
  }

  func show() {
    guard !panel.isVisible else { return }
    if let screen = NSScreen.main?.visibleFrame {
      panel.setFrame(corners(of: screen)[0], display: false)
    }
    panel.orderFrontRegardless()
  }

  /// The test is about to click into `window` (global, top-left origin), or has clicked when
  /// `nil`: the panel moves off the window and lets the click through.
  func clicking(into window: CGRect?) {
    guard let window else {
      panel.ignoresMouseEvents = false
      return
    }
    panel.ignoresMouseEvents = true
    avoid(window)
  }

  /// Moves to the corner that covers least of `window` (global, top-left origin).
  func avoid(_ window: CGRect) {
    guard let primary = NSScreen.screens.first?.frame else { return }
    // Cocoa counts from the bottom of the primary screen.
    let frame = NSRect(x: window.minX, y: primary.maxY - window.maxY, width: window.width, height: window.height)
    guard panel.frame.intersects(frame) else { return }
    let screen = NSScreen.screens.first { $0.frame.contains(NSPoint(x: frame.midX, y: frame.midY)) } ?? NSScreen.main
    guard let visible = screen?.visibleFrame else { return }
    let overlap = { (corner: NSRect) -> CGFloat in
      let common = corner.intersection(frame)
      return common.isNull ? 0 : common.width * common.height
    }
    if let best = corners(of: visible).min(by: { overlap($0) < overlap($1) }) {
      panel.setFrame(best, display: true)
    }
  }

  /// Places for the panel, top right first.
  private func corners(of screen: NSRect) -> [NSRect] {
    let inset = 12.0
    let left = screen.minX + inset, right = screen.maxX - size.width - inset
    let top = screen.maxY - size.height - inset, bottom = screen.minY + inset
    return [(right, top), (left, top), (right, bottom), (left, bottom)].map {
      NSRect(origin: NSPoint(x: $0.0, y: $0.1), size: size)
    }
  }

  func close() {
    panel.orderOut(nil)
  }
}

private final class HUDPanel: NSPanel {
  override var canBecomeKey: Bool { false }
  override var canBecomeMain: Bool { false }
}

/// Buttons react to the first click, without making the panel active first.
private final class FirstMouseHostingView<Content: View>: NSHostingView<Content> {
  override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}

struct RunHUD: View {
  static let width = 336.0
  /// Tall enough for the longest state; the empty part below the HUD lets clicks through.
  static let height = 560.0
  let center: RunCenter
  @AppStorage(RunInputSettings.pausesOnMouseKey) private var pausesOnMouse = true
  @AppStorage(RunInputSettings.escapeStopsKey) private var escapeStops = true
  @State private var valuesShown = false

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      if let state = center.state {
        content(state)
      }
    }
    .padding(16)
    .frame(width: Self.width - 16, alignment: .leading)
    .adaptiveGlass(in: .rect(cornerRadius: 22))
    .padding(8)
    .frame(width: Self.width, height: Self.height, alignment: .top)
    .accessibilityElement(children: .contain)
    .accessibilityLabel(Text("Test run"))
  }

  @ViewBuilder
  private func content(_ state: RunState) -> some View {
    let document = center.running?.document
    let (step, total) = Self.step(of: state, in: document)
    HStack(spacing: 8) {
      let icon = Self.icon(of: state)
      Image(systemName: icon.symbol)
        .font(.title2)
        .foregroundStyle(icon.color)
        .contentTransition(.symbolEffect(.replace))
        .accessibilityHidden(true)
      VStack(alignment: .leading, spacing: 0) {
        Text(state.name.isEmpty ? String(localized: "Untitled Test") : state.name)
          .font(.headline)
          .lineLimit(1)
        Text(center.batch.map { "\(Self.status(of: state)) · \($0.progress)" } ?? Self.status(of: state))
          .font(.subheadline)
          .foregroundStyle(.secondary)
      }
      .accessibilityElement(children: .combine)
      Spacer(minLength: 0)
      Button("Show JEVision", systemImage: "macwindow") { center.showApp() }
        .labelStyle(.iconOnly)
        .buttonStyle(.borderless)
        .help(Text("Show JEVision"))
    }

    VStack(alignment: .leading, spacing: 4) {
      ProgressView(value: Double(min(state.done, total)), total: Double(total))
        .accessibilityHidden(true)
      if state.rows > 0 {
        Text(
          state.rowLabel.isEmpty
            ? "Row \(Int(state.row)) of \(Int(state.rows))"
            : "Row \(Int(state.row)) of \(Int(state.rows)): \(state.rowLabel)"
        )
        .font(.caption.weight(.medium))
        .lineLimit(1)
        .truncationMode(.tail)
      }
      Text("Step \(step) of \(total)")
        .font(.caption)
        .monospacedDigit()
        .foregroundStyle(.secondary)
    }
    .accessibilityElement(children: .combine)

    if let layout = document?.layout, !layout.blocks.isEmpty {
      RunMiniMapView(layout: layout, state: state)
    }

    currentBlock(state, document: document)
    live(state)

    if let choice = state.lastChoice.flatMap(RunText.branch) {
      Label(choice, systemImage: "arrow.triangle.branch")
        .font(.caption)
        .foregroundStyle(.secondary)
        .lineLimit(2)
        .truncationMode(.middle)
    }

    if let batch = center.batch, batch.entries.contains(where: Self.isWaiting) {
      queue(batch)
    }

    if !state.variables.isEmpty {
      values(state.variables)
    }

    if case .waitingForScreen(let holder) = state.phase {
      Label(RunText.screenBusy(holder), systemImage: "hourglass")
        .font(.callout)
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .adaptiveGlass(tint: .orange.opacity(0.35), in: .rect(cornerRadius: 12))
    }

    if state.phase == .paused(.userInput) {
      Label("Paused — you moved the mouse. Resume when your hands are off.", systemImage: "hand.raised.fill")
        .font(.callout)
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .adaptiveGlass(tint: .orange.opacity(0.35), in: .rect(cornerRadius: 12))
    }

    HUDButtonRow {
      if state.isPaused {
        Button("Resume", systemImage: "play.fill", action: center.resume)
          .buttonStyle(HUDButtonStyle(tint: .accentColor))
      } else {
        Button("Pause", systemImage: "pause.fill", action: center.pause)
          .disabled(state.isWaitingForScreen)
      }
      Button("Step", systemImage: "forward.frame.fill", action: center.step)
        .disabled(state.isWaitingForScreen)
        .help(Text("Run one block, then pause"))
    } trailing: {
      Button("Stop", systemImage: "stop.fill", role: .destructive, action: center.stop)
        .buttonStyle(HUDButtonStyle(tint: .red))
    }
    .buttonStyle(HUDButtonStyle())
    .labelStyle(.titleAndIcon)

    hints
  }

  /// What the person's own hands do to the run, as far as Settings let them.
  @ViewBuilder
  private var hints: some View {
    if escapeStops || pausesOnMouse {
      HStack(spacing: 4) {
        if escapeStops {
          Text(verbatim: "esc esc")
            .font(.caption2.monospaced().weight(.semibold))
            .padding(.horizontal, 4)
            .background(.quaternary, in: .rect(cornerRadius: 4))
          Text("stop from anywhere")
        }
        if escapeStops, pausesOnMouse { Text(verbatim: "·") }
        if pausesOnMouse { Text("move the mouse to pause") }
      }
      .font(.caption2)
      .foregroundStyle(.secondary)
      .accessibilityElement(children: .ignore)
      .accessibilityLabel(
        Text(
          [
            escapeStops ? String(localized: "Press Escape twice to stop from any app.") : nil,
            pausesOnMouse ? String(localized: "Moving the mouse pauses the test.") : nil,
          ].compactMap { $0 }.joined(separator: " ")))
    }
  }

  @ViewBuilder
  private func currentBlock(_ state: RunState, document: FlowDocument?) -> some View {
    let block = state.current.flatMap { document?.block($0) }
    let type = block?.kind.typeName ?? state.currentType
    if let type {
      HStack(spacing: 10) {
        IconTile(symbol: BlockInfo.symbol(type), color: BlockInfo.color(ofType: type), size: 30)
        VStack(alignment: .leading, spacing: 1) {
          Text(BlockInfo.title(type))
            .font(.callout.weight(.semibold))
          Text(block.map { BlockSummary.text($0.kind, appName: AppCatalog.shared.name(of:)) } ?? String(localized: "In a test this one runs"))
            .font(.callout)
            .foregroundStyle(.secondary)
            .lineLimit(2)
            .truncationMode(.middle)
        }
      }
      .accessibilityElement(children: .combine)
    }
  }

  /// What the block that runs now waits for and the retry the test is in, if any.
  @ViewBuilder
  private func live(_ state: RunState) -> some View {
    if state.waitLimit != nil || state.attempt > 1 {
      TimelineView(.periodic(from: .now, by: 1)) { context in
        Text(Self.live(of: state, at: context.date))
          .font(.caption)
          .monospacedDigit()
          .foregroundStyle(.secondary)
          .frame(maxWidth: .infinity, alignment: .leading)
          .accessibilityLabel(Text(Self.liveDescription(of: state)))
      }
    }
  }

  /// "6 s left · retry 1 of 2": the time the wait has left, ticking, and the retry; each when it applies.
  static func live(of state: RunState, at now: Date) -> String {
    var parts: [String] = []
    if let left = state.remaining(at: now) {
      parts.append(String(localized: "\(Int(left.rounded(.up))) s left"))
    }
    if state.attempt > 1 { parts.append(retry(of: state)) }
    let line = parts.joined(separator: " · ")
    return line.prefix(1).uppercased() + line.dropFirst()
  }

  /// The same for VoiceOver: what doesn't change from second to second.
  static func liveDescription(of state: RunState) -> String {
    var parts: [String] = []
    if let limit = state.waitLimit {
      let time = Duration.seconds(Int(limit.rounded(.up))).formatted(.units(allowed: [.minutes, .seconds], width: .wide))
      parts.append(String(localized: "Waits up to \(time)"))
    }
    if state.attempt > 1 { parts.append(retry(of: state)) }
    return parts.joined(separator: ", ")
  }

  private static func retry(of state: RunState) -> String {
    String(localized: "retry \(Int(state.attempt) - 1) of \(max(Int(state.maxAttempts) - 1, 1))")
  }

  /// The values the next block sees, folded away until the person wants them; a secret only says
  /// that it is set.
  private func values(_ values: [String: String]) -> some View {
    DisclosureGroup(isExpanded: $valuesShown) {
      ScrollView {
        Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 8, verticalSpacing: 3) {
          ForEach(values.sorted { $0.key < $1.key }, id: \.key) { name, value in
            GridRow {
              Text(verbatim: name)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
              Text(verbatim: Self.shown(name, value))
                .lineLimit(1)
                .truncationMode(.tail)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .accessibilityElement(children: .combine)
          }
        }
        .font(.caption.monospaced())
      }
      .frame(maxHeight: 84)
    } label: {
      Text("Values")
        .font(.caption.weight(.semibold))
        .foregroundStyle(.secondary)
    }
    .accessibilityElement(children: .contain)
    .accessibilityLabel(Text("Values"))
  }

  /// How a value reads in the panel: a secret says only that it is set, whatever the runner sends.
  static func shown(_ name: String, _ value: String) -> String {
    if name.hasPrefix("secret.") { return String(localized: "Set") }
    return value.isEmpty ? "\u{201C}\u{201D}" : value
  }

  /// The tests of the batch running now and the next ones waiting, with what each waits for.
  private func queue(_ batch: RunCenter.Batch) -> some View {
    // The test the sheet holds back hasn't started, though the plan counts it as running.
    let running = batch.entries.indices.filter { index in
      batch.entries[index].state == .running && center.blocked?.position != UInt32(index)
    }.map { batch.entries[$0] }
    let waiting = batch.entries.filter(Self.isWaiting)
    let names = Dictionary(batch.entries.map { ($0.file, $0.name) }, uniquingKeysWith: { first, _ in first })
    return VStack(alignment: .leading, spacing: 3) {
      Text(Self.mode(batch.plan.mode()))
        .font(.caption.weight(.semibold))
        .foregroundStyle(.secondary)
      ForEach(running, id: \.file) { entry in
        Label(entry.name, systemImage: "play.fill")
          .accessibilityLabel(Text("Running: \(entry.name)"))
      }
      ForEach(waiting.prefix(3), id: \.file) { entry in
        Label {
          Text(verbatim: "\(entry.name) · \(Self.waits(entry, names: names))")
        } icon: {
          Image(systemName: "hourglass")
        }
        .accessibilityLabel(Text("Waiting: \(entry.name), \(Self.waits(entry, names: names))"))
      }
      if waiting.count > 3 {
        Text("and \(waiting.count - 3) more waiting")
          .foregroundStyle(.secondary)
      }
    }
    .font(.caption)
    .lineLimit(1)
    .truncationMode(.middle)
    .accessibilityElement(children: .contain)
    .accessibilityLabel(Text("Tests of the run"))
  }

  private static func isWaiting(_ entry: BatchEntry) -> Bool {
    if case .waiting = entry.state { return true }
    return false
  }

  static func mode(_ mode: RunMode) -> String {
    switch mode {
    case .sequential: String(localized: "One after another")
    case .parallel(let limit): String(localized: "Side by side, up to \(Int(limit)) at a time")
    }
  }

  /// What a waiting test waits for, in a few words.
  static func waits(_ entry: BatchEntry, names: [String: String]) -> String {
    guard case .waiting(let reason) = entry.state else { return "" }
    switch reason {
    case .dependency(let test): return String(localized: "after \(names[test] ?? test)")
    case .turn: return String(localized: "next in line")
    case .screen: return String(localized: "waits for the screen")
    case .limit: return String(localized: "waits for a free place")
    }
  }

  /// The block that runs now and how many the test has, counted as "Step N of M" shows them.
  static func step(of state: RunState, in document: FlowDocument?) -> (current: Int, total: Int) {
    let total = max(document?.order.count ?? 0, 1)
    return (min(state.done + 1, total), total)
  }

  /// The header's icon: a run going, one paused, or one held back until another lets go of the screen.
  static func icon(of state: RunState) -> (symbol: String, color: Color) {
    if state.isWaitingForScreen { return ("hourglass.circle.fill", .indigo) }
    if state.isPaused { return ("pause.circle.fill", .orange) }
    return ("play.circle.fill", .green)
  }

  /// What the run is doing, in a few words.
  static func status(of state: RunState) -> String {
    switch state.phase {
    case .running: String(localized: "Running")
    case .waitingForScreen: String(localized: "Waiting for the screen")
    case .paused(.requested): String(localized: "Paused")
    case .paused(.step): String(localized: "Paused before the next block")
    case .paused(.breakpoint): String(localized: "Paused at “Pause Here”")
    case .paused(.userInput): String(localized: "Paused — you moved the mouse")
    case .finished: String(localized: "Finished")
    }
  }
}
