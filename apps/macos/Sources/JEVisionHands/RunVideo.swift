import AVFoundation
import AppKit
import JEVisionCore
import ScreenCaptureKit

/// Records the windows of the app under test into an H.264 `.mp4` while a test runs. Only that
/// app's windows are in the picture — not other apps, notifications or JEVision's own panel. The
/// picture closes in on the app's windows, so their text stays readable on a large display, and
/// follows the app when its windows move or it quits and opens again during the test. Like every
/// screenshot, each frame has the project's hidden areas and the password fields painted over.
final class RunVideo: NSObject, SCStreamOutput, SCStreamDelegate, @unchecked Sendable {
  /// Enough for following clicks and typing; keeps files small.
  private static let frameRate: Int32 = 15
  /// Longest side of the video in pixels: sharp enough to read text, playable everywhere.
  private static let maxSide = 1920.0
  /// How often to check whether the app's windows moved or the app quit or opened again.
  private static let followInterval = DispatchTimeInterval.milliseconds(500)
  /// Room around the app's windows, in points.
  private static let margin = 24.0

  private let app: String
  private let url: URL
  /// Areas covered in every window, in fractions of it from its top-left corner.
  private let hide: [Rect]
  /// Searches the app's windows for password fields; a search can take a quarter of a second.
  private let searching = DispatchQueue(label: "io.github.dev-pikapik.jevision.video.fields")
  /// Every property below is used on this queue only.
  private let queue = DispatchQueue(label: "io.github.dev-pikapik.jevision.video")
  private var stream: SCStream?
  private var display: SCDisplay?
  private var size = (width: 0, height: 0)
  /// The part of the display the picture shows, in the display's points.
  private var area = CGRect.null
  /// Where the picture is moving to while the stream takes its new place.
  private var nextArea: CGRect?
  /// Password fields of each window, in fractions of it, from the latest search.
  private var secure: [CGWindowID: [Rect]] = [:]
  private var isSearching = false
  private var writer: AVAssetWriter?
  private var input: AVAssetWriterInput?
  private var hasPicture = false
  /// Processes of the app the picture shows.
  private var shown: Set<pid_t> = []
  private var following: DispatchSourceTimer?
  private var isUpdating = false

  init(app: String, url: URL, hide: [Rect]) {
    self.app = app
    self.url = url
    self.hide = hide
  }

  /// Starts recording; the video's first moment is the moment of this call. Blocks the calling
  /// thread, which must not be the main one, until the recording runs.
  func start() throws(HandsError) {
    let origin = CMClockGetTime(CMClockGetHostTimeClock())
    let failure = MacHands.wait { (done: @escaping @Sendable (HandsError?) -> Void) in
      Task {
        do {
          try await self.begin(at: origin)
          done(nil)
        } catch let error as HandsError {
          done(error)
        } catch {
          done(.Failed(reason: error.localizedDescription))
        }
      }
    }
    if let failure {
      queue.sync { writer?.cancelWriting() }
      try? FileManager.default.removeItem(at: url)
      throw failure
    }
  }

  /// Stops recording and finishes the file. Without a single picture no file is left.
  func stop() throws(HandsError) {
    let failure = MacHands.wait { (done: @escaping @Sendable (String?) -> Void) in
      let stream = queue.sync {
        following?.cancel()
        following = nil
        return self.stream
      }
      Task {
        // A stream that stopped on its own throws here; what it recorded is still written.
        try? await stream?.stopCapture()
        self.queue.async { self.finish(done) }
      }
    }
    if let failure { throw .Failed(reason: failure) }
  }

  // MARK: Recording

  private func begin(at origin: CMTime) async throws {
    let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
    let apps = content.applications.filter { $0.bundleIdentifier == app }
    guard let display = Self.display(showing: app, in: content) else { throw HandsError.NoWindow(app: app) }
    let filter = SCContentFilter(display: display, including: apps, exceptingWindows: [])
    let size = Self.videoSize(of: display, scale: Double(filter.pointPixelScale))
    let area = Self.framing(MacHands.windows(of: MacHands.runningApps(app)).map(\.bounds), on: display.frame)

    try? FileManager.default.removeItem(at: url)
    let writer = try AVAssetWriter(outputURL: url, fileType: .mp4)
    let input = AVAssetWriterInput(
      mediaType: .video,
      outputSettings: [
        AVVideoCodecKey: AVVideoCodecType.h264,
        AVVideoWidthKey: size.width,
        AVVideoHeightKey: size.height,
        AVVideoCompressionPropertiesKey: [
          AVVideoAverageBitRateKey: size.width * size.height,
          AVVideoExpectedSourceFrameRateKey: Self.frameRate,
          // A key frame every two seconds keeps seeking to a step quick.
          AVVideoMaxKeyFrameIntervalKey: Self.frameRate * 2,
          AVVideoProfileLevelKey: AVVideoProfileLevelH264HighAutoLevel,
        ],
      ])
    input.expectsMediaDataInRealTime = true
    writer.add(input)
    guard writer.startWriting() else {
      throw HandsError.Failed(reason: writer.error?.localizedDescription ?? "the video file can’t be written")
    }
    // Starting at the call's moment, not at the first picture, keeps step times in line with
    // the video even when the app shows its first window later.
    writer.startSession(atSourceTime: origin)

    let stream = SCStream(filter: filter, configuration: Self.configuration(size: size, area: area), delegate: self)
    try stream.addStreamOutput(self, type: .screen, sampleHandlerQueue: queue)
    let secure = Self.secureFields(of: app)
    queue.sync {
      self.secure = secure
      self.writer = writer
      self.input = input
      self.stream = stream
      self.display = display
      self.size = size
      self.area = area
      shown = Set(apps.map(\.processID))
    }
    try await stream.startCapture()
    queue.sync { startFollowing() }
  }

  func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
    guard type == .screen, sampleBuffer.isValid, Self.isComplete(sampleBuffer),
      let input, input.isReadyForMoreMediaData, cover(sampleBuffer)
    else { return }
    if input.append(sampleBuffer) { hasPicture = true }
  }

  /// Paints over the hidden areas and password fields of every window of the app, where the
  /// windows are now. A picture that can't be painted is left out rather than kept as it is.
  private func cover(_ sample: CMSampleBuffer) -> Bool {
    guard !hide.isEmpty || secure.values.contains(where: { !$0.isEmpty }) else { return true }
    guard let display, let picture = sample.imageBuffer else { return false }
    let origin = display.frame.origin
    let windows = MacHands.windows(of: MacHands.runningApps(app)).map {
      VideoWindow(bounds: Rect($0.bounds.offsetBy(dx: -origin.x, dy: -origin.y)), hide: hide + (secure[$0.id] ?? []))
    }
    let mask = videoMask(
      width: UInt32(CVPixelBufferGetWidth(picture)), height: UInt32(CVPixelBufferGetHeight(picture)),
      shown: [area, nextArea].compactMap { $0.map(Rect.init) }, windows: windows)
    return mask.areas.isEmpty || Self.paint(mask, on: picture)
  }

  func stream(_ stream: SCStream, didStopWithError error: any Error) {
    queue.async { [self] in
      following?.cancel()
      following = nil
      self.stream = nil
    }
  }

  private func finish(_ done: @escaping @Sendable (String?) -> Void) {
    stream = nil
    guard let writer, let input else { return done(nil) }
    self.writer = nil
    self.input = nil
    guard hasPicture, writer.status == .writing else {
      writer.cancelWriting()
      try? FileManager.default.removeItem(at: url)
      return done(writer.status == .failed ? writer.error?.localizedDescription : nil)
    }
    input.markAsFinished()
    // The last picture lasts until the end, however long nothing changed on screen.
    writer.endSession(atSourceTime: CMClockGetTime(CMClockGetHostTimeClock()))
    // The writer calls back once it is done with itself; nothing else touches it by then.
    nonisolated(unsafe) let finishing = writer
    writer.finishWriting { [url] in
      if finishing.status == .completed { return done(nil) }
      try? FileManager.default.removeItem(at: url)
      done(finishing.error?.localizedDescription ?? "the video couldn’t be finished")
    }
  }

  // MARK: Following the app

  private func startFollowing() {
    let timer = DispatchSource.makeTimerSource(queue: queue)
    timer.schedule(deadline: .now() + Self.followInterval, repeating: Self.followInterval)
    timer.setEventHandler { [weak self] in
      self?.searchFields()
      self?.follow()
    }
    timer.resume()
    following = timer
  }

  /// Shows the app's new processes once it quit or opened again, and closes in on its windows
  /// where they are now. While the app shows no window the picture stays where it was.
  private func follow() {
    guard !isUpdating, let stream, let display else { return }
    let apps = MacHands.runningApps(app)
    let running = Set(apps.map(\.processIdentifier))
    let windows = MacHands.windows(of: apps).map(\.bounds)
    let area = windows.isEmpty ? self.area : Self.framing(windows, on: display.frame)
    let moves = area != self.area
    guard running != shown || moves else { return }
    isUpdating = true
    let app = self.app
    let config = moves ? Self.configuration(size: size, area: area) : nil
    let rejoins = running != shown
    if moves { nextArea = area }
    Task {
      var follows = false
      if rejoins,
        let content = try? await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
      {
        let apps = content.applications.filter { $0.bundleIdentifier == app }
        // A new process joins the shareable content a moment after launch; until it does, the
        // next check tries again.
        if Set(apps.map(\.processID)) == running {
          let filter = SCContentFilter(display: display, including: apps, exceptingWindows: [])
          follows = (try? await stream.updateContentFilter(filter)) != nil
        }
      }
      var moved = false
      if let config {
        moved = (try? await stream.updateConfiguration(config)) != nil
      }
      let isShown = follows, isMoved = moved
      self.queue.async {
        self.isUpdating = false
        self.nextArea = nil
        if isShown { self.shown = running }
        if isMoved { self.area = area }
      }
    }
  }

  /// Looks for password fields in the app's windows again, off the recording queue.
  private func searchFields() {
    guard !isSearching else { return }
    isSearching = true
    let app = self.app
    searching.async {
      let found = Self.secureFields(of: app)
      self.queue.async {
        self.secure = found
        self.isSearching = false
      }
    }
  }

  // MARK: Helpers

  /// Password fields of each of the app's windows, in fractions of the window.
  private static func secureFields(of app: String) -> [CGWindowID: [Rect]] {
    let apps = MacHands.runningApps(app)
    let pids = Set(apps.map(\.processIdentifier))
    var fields: [CGWindowID: [Rect]] = [:]
    for window in MacHands.windows(of: apps) {
      fields[window.id] = Accessibility.secureFields(pids: pids, window: window.bounds)
    }
    return fields
  }

  /// Paints `mask` over a picture in the stream's two-plane 4:2:0 format: brightness for every
  /// pixel, then one colour pair for every 2×2 pixels. False when the picture has another format.
  static func paint(_ mask: VideoMask, on picture: CVPixelBuffer) -> Bool {
    guard CVPixelBufferLockBaseAddress(picture, []) == kCVReturnSuccess else { return false }
    defer { CVPixelBufferUnlockBaseAddress(picture, []) }
    guard CVPixelBufferGetPlaneCount(picture) == 2,
      let luma = CVPixelBufferGetBaseAddressOfPlane(picture, 0)?.assumingMemoryBound(to: UInt8.self),
      let chroma = CVPixelBufferGetBaseAddressOfPlane(picture, 1)?.assumingMemoryBound(to: UInt8.self)
    else { return false }
    let lumaRow = CVPixelBufferGetBytesPerRowOfPlane(picture, 0)
    let chromaRow = CVPixelBufferGetBytesPerRowOfPlane(picture, 1)
    let (width, height) = (CVPixelBufferGetWidth(picture), CVPixelBufferGetHeight(picture))
    for area in mask.areas {
      let x = Int(area.x), y = Int(area.y)
      let right = min(x + Int(area.width), width), bottom = min(y + Int(area.height), height)
      guard x < right, y < bottom else { continue }
      for row in y..<bottom {
        (luma + row * lumaRow + x).update(repeating: mask.luma, count: right - x)
      }
      // The areas start on even pixels, so a colour pair at byte `x` belongs to pixels x and x+1.
      for row in y / 2..<(bottom + 1) / 2 {
        let line = chroma + row * chromaRow
        for column in stride(from: x & ~1, to: right, by: 2) {
          line[column] = mask.chromaBlue
          line[column + 1] = mask.chromaRed
        }
      }
    }
    return true
  }

  private static func configuration(size: (width: Int, height: Int), area: CGRect) -> SCStreamConfiguration {
    let config = SCStreamConfiguration()
    config.width = size.width
    config.height = size.height
    config.sourceRect = area
    config.minimumFrameInterval = CMTime(value: 1, timescale: frameRate)
    config.pixelFormat = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
    config.showsCursor = true
    return config
  }

  /// The part of `display` (global points) to show for `windows` (global points): all of them
  /// with some room around, in the display's proportions so nothing is stretched, in the display's
  /// own points. The whole display when no window is on it.
  static func framing(_ windows: [CGRect], on display: CGRect) -> CGRect {
    let whole = CGRect(origin: .zero, size: display.size)
    let union = windows.map { $0.intersection(display) }.filter { !$0.isNull && !$0.isEmpty }
      .reduce(CGRect.null) { $0.union($1) }
    guard !union.isNull, display.width > 0, display.height > 0 else { return whole }
    var area = union.insetBy(dx: -margin, dy: -margin)
    let aspect = display.width / display.height
    if area.width / area.height < aspect {
      area = area.insetBy(dx: -(area.height * aspect - area.width) / 2, dy: 0)
    } else {
      area = area.insetBy(dx: 0, dy: -(area.width / aspect - area.height) / 2)
    }
    guard area.width < display.width, area.height < display.height else { return whole }
    area.origin.x = min(max(area.minX, display.minX), display.maxX - area.width)
    area.origin.y = min(max(area.minY, display.minY), display.maxY - area.height)
    return area.offsetBy(dx: -display.minX, dy: -display.minY).integral
  }

  /// The display with the app's front window, or the main display while the app shows none.
  private static func display(showing app: String, in content: SCShareableContent) -> SCDisplay? {
    if let window = MacHands.frontWindow(of: MacHands.runningApps(app)),
      let display = content.displays.first(where: {
        $0.frame.contains(CGPoint(x: window.bounds.midX, y: window.bounds.midY))
      })
    {
      return display
    }
    return content.displays.first { $0.displayID == CGMainDisplayID() } ?? content.displays.first
  }

  /// The display's size in pixels, scaled down to `maxSide`; even numbers, as H.264 needs.
  static func videoSize(of display: SCDisplay, scale: Double) -> (width: Int, height: Int) {
    videoSize(width: Double(display.width) * scale, height: Double(display.height) * scale)
  }

  static func videoSize(width: Double, height: Double) -> (width: Int, height: Int) {
    let fit = min(1, maxSide / max(width, height, 1))
    let even = { (side: Double) in max(2, Int((side * fit / 2).rounded()) * 2) }
    return (even(width), even(height))
  }

  /// Whether the buffer holds a new picture, not a note that nothing changed.
  private static func isComplete(_ buffer: CMSampleBuffer) -> Bool {
    guard
      let attachments = CMSampleBufferGetSampleAttachmentsArray(buffer, createIfNecessary: false)
        as? [[SCStreamFrameInfo: Any]],
      let raw = attachments.first?[.status] as? Int
    else { return false }
    return SCFrameStatus(rawValue: raw) == .complete
  }
}

extension Rect {
  init(_ rect: CGRect) {
    self.init(x: rect.minX, y: rect.minY, width: rect.width, height: rect.height)
  }
}
