import AppKit
import Foundation
import JEVisionCore
import Testing

@testable import JEVisionHands

/// Runs examples/textedit for real: TextEdit opens on the screen, gets typed into, is checked
/// against its reference image and quits. Needs a logged-in session and Screen Recording and
/// Accessibility for the app the tests run from, so it runs only when asked to.
@Suite(
  .serialized,
  .enabled(
    if: ProcessInfo.processInfo.environment["JEVISION_E2E"] == "1",
    "Drives TextEdit on the screen; run with JEVISION_E2E=1 on a Mac with Screen Recording and Accessibility granted"))
struct TextEditEndToEndTests {
  private static let test = "write-a-note.flow.json"
  /// The reference the first run creates; in dark mode the test keeps a dark one beside the light.
  private static let reference = MacHands.systemIsDark ? "refs/note.dark.png" : "refs/note.png"

  @Test func createsTheReferenceThenPassesThenCatchesAChange() throws {
    let project = try Self.copyOfExample()
    defer {
      Self.quitTextEdit()
      try? FileManager.default.removeItem(at: project)
    }

    let first = try Self.run(project)
    try #require(first.report.verdict == .passed)
    #expect(first.report.baselinesCreated == [Self.reference])
    #expect(FileManager.default.fileExists(atPath: project.appending(path: Self.reference).path))
    #expect(first.report.attempts.last?.steps.map(\.status).contains(.baselineCreated) == true)

    let second = try Self.run(project)
    #expect(second.report.verdict == .passed)
    #expect(second.report.baselinesCreated.isEmpty)
    let checked = try #require(second.report.attempts.last?.steps.first { $0.block == "check" })
    #expect(checked.status == .passed)

    try Self.spoil(project.appending(path: Self.reference))
    let third = try Self.run(project)
    Self.quitTextEdit()
    #expect(third.report.verdict == .failed)
    let failed = try #require(third.report.attempts.last?.steps.first { $0.block == "check" })
    #expect(failed.status == .failed)
    #expect(failed.failure?.code == "run.check.mismatch")
    #expect(try #require(failed.similarity) < 0.95)
    let diff = try #require(failed.diff)
    #expect(FileManager.default.fileExists(atPath: third.dir.appending(path: diff).path))

    let json = try JSONSerialization.jsonObject(with: Data(contentsOf: third.dir.appending(path: "report.json")))
    #expect((json as? [String: Any])?["verdict"] as? String == "failed")
    let html = try String(contentsOf: try #require(third.html), encoding: .utf8)
    #expect(html.contains("Failed") && html.contains("Differences") && html.contains("data:image/png;base64,"))
    let junit = try String(contentsOf: third.dir.appending(path: "junit.xml"), encoding: .utf8)
    #expect(junit.contains("<failure"))
  }

  // MARK: Helpers

  private struct Finished {
    let dir: URL
    let html: URL?
    let report: RunReport
  }

  /// The example without a reference image, in a temporary folder, so runs don't touch the repository.
  private static func copyOfExample() throws -> URL {
    let example = URL(filePath: #filePath)
      .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
      .appending(path: "../../examples/textedit").standardizedFileURL
    let project = FileManager.default.temporaryDirectory.appending(path: "jevision-e2e-\(UUID().uuidString)")
    try FileManager.default.copyItem(at: example, to: project)
    try? FileManager.default.removeItem(at: project.appending(path: "refs"))
    try? FileManager.default.removeItem(at: project.appending(path: ".jevision"))
    return project
  }

  private static func run(_ project: URL) throws -> Finished {
    let run = TestRun()
    let listener = Listener()
    run.start(
      project: project.path, test: test, options: RunOptions(startAt: nil, retries: nil, vision: nil, localOnly: false, video: .off, language: "en"),
      hands: MacHands(input: InputWatcher(), isCancelled: { run.isCancelled() }),
      secrets: StoredSecrets(), listener: listener)
    guard listener.done.wait(timeout: .now() + 90) == .success else {
      run.cancel()
      throw Failure("The run didn’t finish in 90 seconds")
    }
    switch listener.outcome {
    case .finished(let dir, let html, let report)?:
      if let failure = report.attempts.last?.failure, report.verdict != .failed || failure.code != "run.check.mismatch" {
        Issue.record("The run stopped: \(failure.code) \(failure.params)")
      }
      return Finished(dir: URL(filePath: dir), html: html.map { URL(filePath: $0) }, report: report)
    case .failed(let reason)?: throw Failure(reason)
    case nil: throw Failure("No outcome")
    }
  }

  /// Paints a dark band across the reference, as if the app had changed.
  private static func spoil(_ url: URL) throws {
    let source = try #require(CGImageSourceCreateWithURL(url as CFURL, nil))
    let image = try #require(CGImageSourceCreateImageAtIndex(source, 0, nil))
    let context = try #require(
      CGContext(
        data: nil, width: image.width, height: image.height, bitsPerComponent: 8, bytesPerRow: 0,
        space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
    context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
    context.setFillColor(CGColor(red: 0.8, green: 0.1, blue: 0.1, alpha: 1))
    context.fill(CGRect(x: 0, y: image.height / 3, width: image.width, height: image.height / 3))
    let spoiled = try #require(context.makeImage())
    try #require(MacHands.png(spoiled)).write(to: url)
  }

  private static func quitTextEdit() {
    for app in NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.TextEdit") {
      app.terminate()
    }
  }

  private struct Failure: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
  }
}

private final class Listener: RunListener, @unchecked Sendable {
  let done = DispatchSemaphore(value: 0)
  var outcome: RunOutcome?

  func event(event: RunEvent) {}

  func finished(outcome: RunOutcome) {
    self.outcome = outcome
    done.signal()
  }
}
