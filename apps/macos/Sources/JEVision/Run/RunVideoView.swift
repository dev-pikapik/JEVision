import AVKit
import JEVisionCore
import SwiftUI

/// The video of a run with a mark for each step on a track under it. A mark, or a step chosen in
/// the list, shows the moment that step began.
struct RunVideoView: View {
  let player: AVPlayer
  /// Steps of the try on screen.
  let steps: [Step]
  @State private var duration: Double?
  @State private var size = CGSize(width: 16, height: 10)
  @State private var time = 0.0

  /// The run's video, when the run kept one and it is still there.
  static func file(of run: SavedRun) -> URL? {
    guard let name = run.report.video else { return nil }
    let url = run.url.appending(path: name)
    return FileManager.default.fileExists(atPath: url.path) ? url : nil
  }

  /// Shows the moment the step began.
  static func seek(_ player: AVPlayer, to step: Step) {
    guard let ms = step.videoAtMs else { return }
    player.seek(to: CMTime(value: CMTimeValue(ms), timescale: 1000), toleranceBefore: .zero, toleranceAfter: .zero)
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      Text("Video")
        .font(.headline)
      PlayerView(player: player)
        .aspectRatio(size, contentMode: .fit)
        .frame(maxHeight: 360)
        .frame(maxWidth: .infinity)
        .clipShape(.rect(cornerRadius: 10))
      if let duration, duration > 0 {
        track(duration)
      }
    }
    .padding(12)
    .background(.background, in: .rect(cornerRadius: 14))
    .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(.separator))
    .task(id: ObjectIdentifier(player)) { await load() }
    .task(id: ObjectIdentifier(player)) {
      while !Task.isCancelled {
        time = player.currentTime().seconds
        try? await Task.sleep(for: .milliseconds(200))
      }
    }
  }

  private func track(_ duration: Double) -> some View {
    let marked = steps.filter { $0.videoAtMs != nil }
    return HStack(spacing: 8) {
      GeometryReader { geometry in
        let width = geometry.size.width
        let x = { (seconds: Double) in width * min(max(seconds / duration, 0), 1) }
        ZStack(alignment: .leading) {
          Capsule()
            .fill(.quaternary)
            .frame(height: 4)
          Capsule()
            .fill(.tint)
            .frame(width: x(time), height: 4)
          ForEach(marked, id: \.seq) { step in
            let failed = step.status == .failed
            Button {
              Self.seek(player, to: step)
            } label: {
              Image(systemName: failed ? RunStatus.failed.dot : "circle.fill")
                .foregroundStyle(failed ? RunStatus.failed.color : Color.secondary)
                .font(failed ? .caption : .caption2)
                .padding(5)
                .contentShape(.rect)
            }
            .buttonStyle(.plain)
            .position(x: x(Double(step.videoAtMs ?? 0) / 1000), y: geometry.size.height / 2)
            .help(Text("Step \(step.seq): \(BlockInfo.title(step.blockType))"))
            .accessibilityLabel(Text("Step \(step.seq): \(BlockInfo.title(step.blockType))"))
            .accessibilityHint(Text("Shows the moment this step began"))
          }
        }
      }
      .frame(height: 20)
      .unmirrored()
      Text(verbatim: "\(Self.clock(time)) / \(Self.clock(duration))")
        .font(.caption.monospacedDigit())
        .foregroundStyle(.secondary)
        .accessibilityHidden(true)
    }
  }

  private func load() async {
    guard let asset = player.currentItem?.asset else { return }
    if let length = try? await asset.load(.duration) { duration = length.seconds }
    if let track = try? await asset.loadTracks(withMediaType: .video).first,
      let natural = try? await track.load(.naturalSize), natural.width > 0, natural.height > 0
    {
      size = natural
    }
  }

  /// `m:ss` of a moment in the video.
  nonisolated static func clock(_ seconds: Double) -> String {
    let whole = Int(seconds.isFinite ? max(seconds, 0) : 0)
    return String(format: "%d:%02d", whole / 60, whole % 60)
  }
}

/// AppKit's player. SwiftUI's `VideoPlayer` aborts the app on macOS 27 while its view is made.
private struct PlayerView: NSViewRepresentable {
  let player: AVPlayer

  func makeNSView(context: Context) -> AVPlayerView {
    let view = AVPlayerView()
    view.controlsStyle = .inline
    view.player = player
    return view
  }

  func updateNSView(_ view: AVPlayerView, context: Context) {
    if view.player !== player { view.player = player }
  }
}
