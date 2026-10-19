import AppKit
import JEVisionCore
import JEVisionHands
import SwiftUI

/// The glass panel shown while recording: what has been recorded so far and the controls.
struct RecordHUD: View {
  static let width = 336.0
  let center: RecordCenter

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      header
      lastStep
        .frame(maxWidth: .infinity, minHeight: 44, maxHeight: 44, alignment: .leading)
      VStack(spacing: 8) {
        HStack(spacing: 8) {
          Button("Add Check", systemImage: "viewfinder", action: center.addCheck)
            .help(Text("Drag a frame around the part of the screen the test should check"))
          Spacer(minLength: 0)
          Button("Done", systemImage: "checkmark", action: center.done)
            .buttonStyle(.glassProminent)
            .help(Text("Stop recording and add the steps to the test"))
        }
        HStack(spacing: 8) {
          Button("Undo Last Step", systemImage: "arrow.uturn.backward", action: center.undoLastStep)
            .disabled(!center.steps.contains { if case .openApp = $0 { false } else { true } })
          Spacer(minLength: 0)
          if center.isPaused {
            Button("Resume", systemImage: "record.circle", action: center.togglePause)
          } else {
            Button("Pause", systemImage: "pause.fill", action: center.togglePause)
              .help(Text("What you do while paused isn’t recorded"))
          }
        }
      }
      .buttonStyle(.glass)
      .labelStyle(.titleAndIcon)

      HStack(spacing: 4) {
        Text(verbatim: "esc esc")
          .font(.caption2.monospaced().weight(.semibold))
          .padding(.horizontal, 4)
          .background(.quaternary, in: .rect(cornerRadius: 4))
        Text("finish from any app")
      }
      .font(.caption2)
      .foregroundStyle(.secondary)
      .accessibilityElement(children: .ignore)
      .accessibilityLabel(Text("Press Escape twice in any app to finish. Clicks on this panel aren’t recorded."))
    }
    .padding(16)
    .frame(width: Self.width - 16, alignment: .leading)
    .adaptiveGlass(in: .rect(cornerRadius: 22))
    .padding(8)
    .fixedSize(horizontal: false, vertical: true)
    .accessibilityElement(children: .contain)
    .accessibilityLabel(Text("Recording"))
  }

  private var header: some View {
    HStack(spacing: 8) {
      Image(systemName: center.isPaused ? "pause.circle.fill" : "record.circle.fill")
        .font(.title2)
        .foregroundStyle(center.isPaused ? .orange : .red)
        .symbolEffect(.pulse, isActive: !center.isPaused)
        .contentTransition(.symbolEffect(.replace))
        .accessibilityHidden(true)
      VStack(alignment: .leading, spacing: 0) {
        Text(center.isPaused ? String(localized: "Paused") : String(localized: "Recording in \(center.appName)"))
          .font(.headline)
          .lineLimit(1)
        Text("\(center.steps.count) steps")
          .font(.subheadline)
          .monospacedDigit()
          .foregroundStyle(.secondary)
      }
      .accessibilityElement(children: .combine)
      Spacer(minLength: 0)
    }
  }

  @ViewBuilder
  private var lastStep: some View {
    if let notice = center.notice {
      Label {
        Text(Self.text(of: notice))
          .lineLimit(2)
      } icon: {
        Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
      }
      .font(.callout)
    } else if let step = center.steps.last {
      let kind = Recording.preview(step)
      HStack(spacing: 10) {
        IconTile(symbol: BlockInfo.symbol(kind.typeName), color: BlockInfo.color(ofType: kind.typeName), size: 30)
        VStack(alignment: .leading, spacing: 1) {
          Text(BlockInfo.title(kind.typeName))
            .font(.callout.weight(.semibold))
          Text(Self.text(of: step, appName: center.appName))
            .font(.callout)
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .truncationMode(.middle)
        }
      }
      .accessibilityElement(children: .combine)
      .accessibilityLabel(Text("Last step: \(Self.text(of: step, appName: center.appName))"))
    } else {
      Text("Click, drag, scroll, type and press keys in \(center.appName). Each action becomes a step.")
        .font(.callout)
        .foregroundStyle(.secondary)
        .lineLimit(2)
    }
  }

  /// A recorded step in a few words, the way the canvas puts it.
  static func text(of step: RecordedStep, appName: String) -> String {
    if case .openApp = step { return String(localized: "Open \(appName)") }
    if case .waitFor = step { return String(localized: "Wait for the new screen") }
    return BlockSummary.text(Recording.preview(step), appName: AppCatalog.shared.name(of:))
  }

  static func text(of notice: Recorder.Notice) -> String {
    switch notice {
    case .outsideWindow:
      String(localized: "Clicks outside the app’s window, like in its menus, can’t be recorded. Use a keyboard shortcut instead.")
    case .dragOutsideWindow:
      String(localized: "Drags that end outside the app’s window can’t be recorded. Drop inside the window.")
    case .noWindow: String(localized: "JEVision can’t see the app’s window. Bring it to the front.")
    case .checkOutsideWindow: String(localized: "Drag over a part of the app’s window to check it.")
    }
  }
}

/// Covers every screen so the person can drag a frame around the part to check. Esc cancels.
@MainActor
final class AreaPicker {
  private var panels: [NSPanel] = []
  private let done: (CGRect?) -> Void

  /// `done` gets the area in global points from the top-left corner, or `nil` when cancelled.
  init(done: @escaping (CGRect?) -> Void) {
    self.done = done
    let top = NSScreen.screens.first?.frame.maxY ?? 0
    for screen in NSScreen.screens {
      let frame = screen.frame
      let panel = KeyPanel(contentRect: frame, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
      panel.level = .screenSaver
      panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .ignoresCycle]
      panel.isOpaque = false
      panel.backgroundColor = .clear
      panel.hasShadow = false
      panel.hidesOnDeactivate = false
      // Shown while JEVision hides for the recording.
      panel.canHide = false
      panel.isReleasedWhenClosed = false
      panel.onCancel = { [weak self] in self?.finish(nil) }
      panel.contentView = NSHostingView(
        rootView: AreaPickerView { [weak self] rect in
          self?.finish(rect.offsetBy(dx: frame.minX, dy: top - frame.maxY))
        })
      panel.setFrame(frame, display: false)
      panels.append(panel)
    }
    for panel in panels { panel.orderFrontRegardless() }
    let mouse = NSEvent.mouseLocation
    (panels.first { $0.frame.contains(mouse) } ?? panels.first)?.makeKey()
  }

  func cancel() { finish(nil) }

  private func finish(_ area: CGRect?) {
    guard !panels.isEmpty else { return }
    for panel in panels { panel.orderOut(nil) }
    panels = []
    done(area)
  }
}

/// Takes the keyboard without making JEVision the active app, so Esc reaches it.
private final class KeyPanel: NSPanel {
  var onCancel: (() -> Void)?
  override var canBecomeKey: Bool { true }
  override func cancelOperation(_ sender: Any?) { onCancel?() }
}

private struct AreaPickerView: View {
  let picked: (CGRect) -> Void
  @State private var start: CGPoint?
  @State private var end: CGPoint?

  private var area: CGRect? {
    guard let start, let end else { return nil }
    return CGRect(x: min(start.x, end.x), y: min(start.y, end.y), width: abs(end.x - start.x), height: abs(end.y - start.y))
  }

  var body: some View {
    ZStack(alignment: .top) {
      Color.black.opacity(0.25)
        .overlay {
          if let area {
            Rectangle()
              .frame(width: area.width, height: area.height)
              .position(x: area.midX, y: area.midY)
              .blendMode(.destinationOut)
          }
        }
        .compositingGroup()
      if let area {
        Rectangle()
          .strokeBorder(.white, style: StrokeStyle(lineWidth: 2, dash: [6, 4]))
          .frame(width: area.width, height: area.height)
          .position(x: area.midX, y: area.midY)
      }
      VStack(spacing: 2) {
        Text("Drag over the part of the screen the test should check")
          .font(.headline)
        Text("Esc cancels")
          .font(.callout)
          .foregroundStyle(.secondary)
      }
      .padding(.horizontal, 18)
      .padding(.vertical, 10)
      .adaptiveGlass(in: .capsule)
      .padding(.top, 60)
      .allowsHitTesting(false)
    }
    .contentShape(.rect)
    .pointerStyle(.rectSelection)
    .gesture(
      DragGesture(minimumDistance: 2)
        .onChanged { value in
          start = value.startLocation
          end = value.location
        }
        .onEnded { _ in
          if let area { picked(area) }
          start = nil
          end = nil
        }
    )
    .accessibilityElement(children: .combine)
    .accessibilityAddTraits(.isModal)
  }
}
