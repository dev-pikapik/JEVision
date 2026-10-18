import AppKit
import JEVisionCore
import SwiftUI

/// The floating glass panel at the top right that shows a run while the window is away. It
/// stays above every app and never takes the focus from the app under test.
@MainActor
final class RunPanel {
  static let width = 336.0
  /// Tall enough for the longest state; the empty part below the HUD lets clicks through.
  static let height = 340.0

  private let panel: NSPanel

  init(center: RunCenter) {
    panel = HUDPanel(
      contentRect: NSRect(x: 0, y: 0, width: Self.width, height: Self.height),
      styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
    panel.level = .statusBar
    panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .ignoresCycle]
    panel.isOpaque = false
    panel.backgroundColor = .clear
    panel.hasShadow = false
    panel.hidesOnDeactivate = false
    panel.isReleasedWhenClosed = false
    panel.contentView = FirstMouseHostingView(rootView: RunHUD(center: center))
  }

  func show() {
    guard !panel.isVisible else { return }
    if let screen = NSScreen.main?.visibleFrame {
      panel.setFrameTopLeftPoint(NSPoint(x: screen.maxX - Self.width - 12, y: screen.maxY - 12))
    }
    panel.orderFrontRegardless()
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
  let center: RunCenter

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      if let state = center.state {
        content(state)
      }
    }
    .padding(16)
    .frame(width: RunPanel.width - 16, alignment: .leading)
    .glassEffect(.regular, in: .rect(cornerRadius: 22))
    .padding(8)
    .frame(width: RunPanel.width, height: RunPanel.height, alignment: .top)
    .accessibilityElement(children: .contain)
    .accessibilityLabel(Text("Test run"))
  }

  @ViewBuilder
  private func content(_ state: RunState) -> some View {
    let document = center.running?.document
    let total = max(document?.order.count ?? 0, 1)
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
        Text(Self.status(of: state))
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
      HStack {
        Text("Step \(min(state.done + 1, total)) of \(total)")
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

    if state.phase == .paused(.userInput) {
      Label("Paused — you moved the mouse. Resume when your hands are off.", systemImage: "hand.raised.fill")
        .font(.callout)
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassEffect(.regular.tint(.orange.opacity(0.35)), in: .rect(cornerRadius: 12))
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
