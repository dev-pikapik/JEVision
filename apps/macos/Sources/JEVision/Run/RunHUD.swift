import AppKit
import JEVisionCore
import SwiftUI

/// A floating glass panel shown while the window is away: during a run or a recording. It stays
/// above every app, never takes the focus from the app under test and keeps to a corner of the
/// screen that window doesn't cover.
@MainActor
final class GlassPanel {
  private let panel: NSPanel
  private let size: NSSize

  /// `height` defaults to what `content` needs.
  init(_ content: some View, width: Double, height: Double? = nil) {
    let view = FirstMouseHostingView(rootView: content)
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
  static let height = 460.0
  let center: RunCenter

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
      Image(systemName: state.isPaused ? "pause.circle.fill" : "play.circle.fill")
        .font(.title2)
        .foregroundStyle(state.isPaused ? .orange : .green)
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
      HStack {
        Text("Step \(step) of \(total)")
        Spacer()
        if state.maxAttempts > 1 {
          Text("Try \(state.attempt) of \(state.maxAttempts)")
        }
      }
      .font(.caption)
      .monospacedDigit()
      .foregroundStyle(.secondary)
    }
    .accessibilityElement(children: .combine)

    currentBlock(state, document: document)

    if state.isPaused, !state.variables.isEmpty {
      variables(state.variables)
    }

    if state.phase == .paused(.userInput) {
      Label("Paused — you moved the mouse. Resume when your hands are off.", systemImage: "hand.raised.fill")
        .font(.callout)
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .adaptiveGlass(tint: .orange.opacity(0.35), in: .rect(cornerRadius: 12))
    }

    HStack(spacing: 8) {
      if state.isPaused {
        Button("Resume", systemImage: "play.fill", action: center.resume)
          .buttonStyle(.glassProminent)
      } else {
        Button("Pause", systemImage: "pause.fill", action: center.pause)
          .buttonStyle(.glass)
      }
      Button("Step", systemImage: "forward.frame.fill", action: center.step)
        .buttonStyle(.glass)
        .help(Text("Run one block, then pause"))
      Spacer(minLength: 0)
      Button("Stop", systemImage: "stop.fill", role: .destructive, action: center.stop)
        .buttonStyle(.glass)
        .tint(.red)
    }
    .labelStyle(.titleAndIcon)

    HStack(spacing: 4) {
      Text(verbatim: "esc esc")
        .font(.caption2.monospaced().weight(.semibold))
        .padding(.horizontal, 4)
        .background(.quaternary, in: .rect(cornerRadius: 4))
      Text("stop from anywhere · move the mouse to pause")
    }
    .font(.caption2)
    .foregroundStyle(.secondary)
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(Text("Press Escape twice to stop from any app. Moving the mouse pauses the test."))
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

  /// The values the next block sees, while the test waits.
  private func variables(_ values: [String: String]) -> some View {
    VStack(alignment: .leading, spacing: 4) {
      Text("Variables")
        .font(.caption.weight(.semibold))
        .foregroundStyle(.secondary)
      ScrollView {
        Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 8, verticalSpacing: 3) {
          ForEach(values.sorted { $0.key < $1.key }, id: \.key) { name, value in
            GridRow {
              Text(verbatim: name)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
              Text(verbatim: value.isEmpty ? "\u{201C}\u{201D}" : value)
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
    }
    .accessibilityElement(children: .contain)
    .accessibilityLabel(Text("Variables"))
  }

  /// The block that runs now and how many the test has, counted as "Step N of M" shows them.
  static func step(of state: RunState, in document: FlowDocument?) -> (current: Int, total: Int) {
    let total = max(document?.order.count ?? 0, 1)
    return (min(state.done + 1, total), total)
  }

  /// What the run is doing, in a few words.
  static func status(of state: RunState) -> String {
    switch state.phase {
    case .running: String(localized: "Running")
    case .paused(.requested): String(localized: "Paused")
    case .paused(.step): String(localized: "Paused before the next block")
    case .paused(.breakpoint): String(localized: "Paused at “Pause Here”")
    case .paused(.userInput): String(localized: "Paused — you moved the mouse")
    case .finished: String(localized: "Finished")
    }
  }
}
