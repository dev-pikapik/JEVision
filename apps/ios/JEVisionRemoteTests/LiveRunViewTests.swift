import SwiftUI
import Testing
import UIKit

@testable import JEVisionRemote

/// The card on top of a live run, in the largest text size, while the run changes state.
@MainActor
struct LiveRunViewTests {
  /// When the card gets shorter, nothing may be drawn where its taller self was: not its old
  /// text fading out at its old size, nor the card shrinking slowly behind the new text. The
  /// screen is pictured many times while each change takes effect, and every picture of the
  /// strip the card gave up has to match the screen either before or after the change.
  @Test func theCardKeepsItsTextInsideWhenTheRunChanges() async throws {
    let mac = HostRecord(hostId: String(repeating: "a", count: 64), name: "Test Mac", deviceId: "d1", relayUrl: nil, addrs: [])
    let (events, feed) = AsyncThrowingStream<RunUpdate, any Error>.makeStream()
    let model = AppModel(client: OneSession(session: FakeSession(record: mac, events: events)), store: makeStore(), lock: makeLock())
    _ = try await model.pair(try #require(PairingLink(pairingText())), deviceName: "iPhone")

    let scene = try #require(UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first)
    let window = UIWindow(windowScene: scene)
    window.rootViewController = UIHostingController(
      rootView: NavigationStack { LiveRunView(host: mac, project: "Acme", run: "run-1") }
        .environment(model)
        .dynamicTypeSize(.accessibility5))
    window.makeKeyAndVisible()
    defer { window.isHidden = true }

    var waiting = RunStatus(run: "run-1", test: "greet.flow.json", state: .running, verdict: nil, attempt: 1, block: nil)
    waiting.waiting = ScreenWait(holder: ScreenHolder(process: "jevision", test: "Sign in", since: .now))
    var running = waiting
    running.waiting = nil
    var passed = running
    passed.state = .finished
    passed.verdict = .passed

    feed.yield(.status(waiting))
    try await Task.sleep(for: .seconds(3))
    let top = try #require(contentTop(of: window))
    var before = try #require(picture(of: window).cgImage)
    for (name, status) in [("running", running), ("passed", passed)] {
      feed.yield(.status(status))
      var frames: [UIImage] = []
      let start = ContinuousClock.now
      while ContinuousClock.now - start < .seconds(1.2) {
        frames.append(picture(of: window))
        try await Task.sleep(for: .milliseconds(40))
      }
      try await Task.sleep(for: .seconds(1.5))
      let after = try #require(picture(of: window).cgImage)

      let scale = Int(window.screen.scale)
      let strip = cardBottom(after, from: top, scale: scale)..<cardBottom(before, from: top, scale: scale)
      try #require(!strip.isEmpty, "The card didn't get shorter on \(name)")
      let old = lightness(before, rows: strip)
      let new = lightness(after, rows: strip)
      var changed: Int?
      var worst = (strays: 0, frame: frames.count - 1)
      for (index, frame) in frames.enumerated() {
        let seen = lightness(try #require(frame.cgImage), rows: strip)
        if changed == nil, differing(seen, old) > seen.count / 1000 { changed = index }
        let strays = zip(differs(seen, old), differs(seen, new)).filter { $0 && $1 }.count
        if strays > worst.strays { worst = (strays, index) }
      }
      Attachment.record(frames[worst.strays > 0 ? worst.frame : changed ?? worst.frame], named: name, as: .png)
      #expect(changed != nil, "The card didn't change on \(name) within the time pictured")
      #expect(
        worst.strays <= old.count / 1000,
        "Under the card, \(worst.strays) of \(old.count) pixels are neither the screen before nor after \(name)")
      before = after
    }
  }

  /// The top of the screen's content, a little inside the card: under the navigation bar and
  /// the content's margin. In pixels.
  private func contentTop(of window: UIWindow) -> Int? {
    func bar(in controller: UIViewController) -> UINavigationBar? {
      (controller as? UINavigationController)?.navigationBar ?? controller.children.lazy.compactMap(bar).first
    }
    guard let root = window.rootViewController, let bar = bar(in: root) else { return nil }
    return Int((bar.convert(bar.bounds, to: nil).maxY + 24) * window.screen.scale)
  }

  /// Where the card ends: the first row under `top` that starts eight points of rows in the
  /// colour of the screen's margin.
  private func cardBottom(_ image: CGImage, from top: Int, scale: Int) -> Int {
    let width = image.width
    let rows = lightness(image, rows: top..<image.height)
    let margin = rows[2 * scale]
    let band = 8 * scale
    var empty = 0
    for row in 0..<rows.count / width {
      let line = rows[row * width..<(row + 1) * width]
      empty = line.allSatisfy { abs(Int($0) - Int(margin)) <= 10 } ? empty + 1 : 0
      if empty == band { return top + row - band + 1 }
    }
    return image.height
  }
}

/// Hands out one session, for pairing and for every connection.
private struct OneSession: RemoteClient {
  let session: FakeSession

  func pair(offer: String, deviceName: String) async throws(RemoteError) -> any MacSession { session }
  func connect(_ host: HostRecord) async throws(RemoteError) -> any MacSession { session }
}

/// A picture of everything the window shows.
@MainActor
func picture(of window: UIWindow) -> UIImage {
  UIGraphicsImageRenderer(bounds: window.bounds).image { _ in
    _ = window.drawHierarchy(in: window.bounds, afterScreenUpdates: false)
  }
}

/// The lightness of every pixel in `rows`, row after row.
func lightness(_ image: CGImage, rows: Range<Int>) -> [UInt8] {
  let width = image.width
  var values = [UInt8](repeating: 0, count: width * rows.count)
  guard let band = image.cropping(to: CGRect(x: 0, y: rows.lowerBound, width: width, height: rows.count)) else {
    return values
  }
  values.withUnsafeMutableBytes { buffer in
    let context = CGContext(
      data: buffer.baseAddress, width: width, height: rows.count, bitsPerComponent: 8, bytesPerRow: width,
      space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue)
    context?.draw(band, in: CGRect(x: 0, y: 0, width: width, height: rows.count))
  }
  return values
}

func differs(_ a: [UInt8], _ b: [UInt8]) -> [Bool] {
  zip(a, b).map { abs(Int($0) - Int($1)) > 24 }
}

func differing(_ a: [UInt8], _ b: [UInt8]) -> Int {
  differs(a, b).filter { $0 }.count
}
