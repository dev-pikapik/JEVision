import SwiftUI
import Testing
import UIKit

@testable import JEVisionRemote

/// The hint under the camera, in the largest text size, when the camera sees another code.
@MainActor
struct ScanHintTests {
  /// The hint changes at once: every picture taken while it changes shows the old hint or the
  /// new one, never the new text spilling out of a card still growing toward it.
  @Test func theHintKeepsItsTextInsideWhenTheCodeIsAnotherOne() async throws {
    let scene = try #require(UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first)
    let window = UIWindow(windowScene: scene)
    let screen = UIHostingController(rootView: hint(otherCode: false))
    window.rootViewController = screen
    window.makeKeyAndVisible()
    defer { window.isHidden = true }
    try await Task.sleep(for: .seconds(1))
    let before = try #require(picture(of: window).cgImage)

    screen.rootView = hint(otherCode: true)
    var frames: [UIImage] = []
    let start = ContinuousClock.now
    while ContinuousClock.now - start < .seconds(1.2) {
      frames.append(picture(of: window))
      try await Task.sleep(for: .milliseconds(40))
    }
    try await Task.sleep(for: .seconds(1.5))
    let after = try #require(picture(of: window).cgImage)

    let everything = 0..<before.height
    let changed = differs(lightness(before, rows: everything), lightness(after, rows: everything))
    let first = try #require(changed.firstIndex(of: true), "The hint didn't change")
    let last = try #require(changed.lastIndex(of: true))
    let rows = first / before.width..<last / before.width + 1
    let old = lightness(before, rows: rows)
    let new = lightness(after, rows: rows)
    var worst = (strays: 0, frame: frames.count - 1)
    for (index, frame) in frames.enumerated() {
      let seen = lightness(try #require(frame.cgImage), rows: rows)
      let strays = zip(differs(seen, old), differs(seen, new)).filter { $0 && $1 }.count
      if strays > worst.strays { worst = (strays, index) }
    }
    Attachment.record(frames[worst.frame], named: "other", as: .png)
    #expect(
      worst.strays <= old.count / 1000,
      "Where the hint changes, \(worst.strays) of \(old.count) pixels are neither the old hint nor the new one")
  }

  /// The hint where the scanner shows it: at the bottom, in from the edges.
  private func hint(otherCode: Bool) -> some View {
    VStack {
      Spacer()
      ScanHint(otherCode: otherCode)
    }
    .padding(.horizontal, 24)
    .padding(.bottom, 12)
    .dynamicTypeSize(.accessibility5)
  }
}
