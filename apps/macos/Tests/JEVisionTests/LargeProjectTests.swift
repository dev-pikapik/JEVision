import Foundation
import JEVisionCore
import Testing

@testable import JEVision

/// What the window works out on every redraw stays quick in a test of hundreds of blocks and a
/// project with a long run history, and nothing breaks in an empty project. Bounds are generous
/// for a debug build on a slow machine.
@MainActor
struct LargeProjectTests {
  private func folder() throws -> URL {
    let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    return folder
  }

  /// A test of `count` blocks in a row; every tenth is an If whose both ways lead on.
  private func longTest(_ count: Int) -> String {
    let blocks = (0..<count).map { i in
      let next = i + 1 < count ? "\"b\(i + 1)\"" : "null"
      if i % 10 == 5 {
        return "\"b\(i)\": {\"type\": \"if\", \"reference\": {\"image\": \"refs/x.png\"}, \"then\": \(next), \"else\": \(next)}"
      }
      return "\"b\(i)\": {\"type\": \"pressKeys\", \"keys\": \"\(i % 2 == 0 ? "" : "a")\", \"next\": \(next)}"
    }
    return "{\"format\": 1, \"name\": \"Long\", \"app\": \"com.example.App\", \"start\": \"b0\", \"blocks\": {\(blocks.joined(separator: ","))}}"
      .replacingOccurrences(of: ", \"next\": null", with: "")
      .replacingOccurrences(of: "\"then\": null, \"else\": null", with: "\"then\": null")
  }

  private func seconds(_ work: () -> Void) -> Double {
    let clock = ContinuousClock()
    let took = clock.measure(work)
    return Double(took.components.seconds) + Double(took.components.attoseconds) / 1e18
  }

  @Test func theCanvasLooksUpBlocksWithoutSearchingTheTest() throws {
    let url = try folder().appending(path: "long.flow.json")
    try longTest(400).write(to: url, atomically: true, encoding: .utf8)
    let document = FlowDocument(url: url)
    let order = document.order
    #expect(order.count == 400)
    #expect(order.enumerated().allSatisfy { document.step(of: $1) == $0 + 1 })
    #expect(document.block("b7")?.id == "b7")
    #expect(document.block("missing") == nil)
    #expect(document.hasLinks("b0") && document.hasLinks("b399"))
    // Half the blocks press no keys: each has a problem of its own.
    #expect(document.issues(of: "b0").count == 1)
    #expect(document.issues(of: "b1").isEmpty)

    // What a redraw of the canvas asks for every block, a hundred times over.
    let took = seconds {
      for _ in 0..<100 {
        for id in order {
          _ = document.block(id)
          _ = document.issues(of: id)
          _ = document.step(of: id)
          _ = document.hasLinks(id)
        }
      }
    }
    #expect(took < 2, "took \(took) s")
  }

  @Test func aLongRunHistoryGroupsQuickly() {
    let runs = (0..<3000).map { n in
      let time = String(format: "2026-%02d-%02dT10:%02d:00.000Z", 1 + n % 12, 1 + n % 28, n % 60)
      return SavedRun(
        dir: "/p/.jevision/runs/\(n)",
        report: RunReport(
          test: "Test \(n % 500)", file: "t\(n % 500).flow.json", app: "com.example.App", startedAt: time,
          durationMs: 1500, verdict: .passed, flaky: false, baselinesCreated: [], attempts: []))
    }
    var days: [RunHistory.Day] = []
    var latest: [String: SavedRun] = [:]
    let took = seconds {
      days = RunHistory.days(runs)
      _ = RunHistory.days(runs, matching: "Test 4")
      latest = ProjectMap.latestRuns(runs)
    }
    #expect(days.flatMap(\.runs).count == 3000)
    #expect(latest.count == 500)
    #expect(took < 2, "took \(took) s")
  }

  @Test func anEmptyProjectShowsNothingAndBreaksNothing() throws {
    let root = try folder()
    #expect(try ProjectScanner.scan(root).isEmpty)
    #expect(ProjectScanner.tests(in: [], matching: "login").isEmpty)
    #expect(ProjectMap.groups([], rootTitle: "Empty").isEmpty)
    #expect(ProjectMap.links([:], bounds: [:]).isEmpty)
    #expect(ProjectMap.usedBy([:]).isEmpty)
    #expect(ProjectMap.latestRuns([]).isEmpty)
    #expect(RunHistory.days([]).isEmpty)

    let url = root.appending(path: "empty.flow.json")
    try #"{"format": 1, "name": "Empty", "app": "com.example.App", "start": "", "blocks": {}}"#
      .write(to: url, atomically: true, encoding: .utf8)
    let document = FlowDocument(url: url)
    #expect(document.flow?.blocks.isEmpty == true)
    #expect(document.order.isEmpty)
    #expect(document.step(of: "a") == 1)
    #expect(!document.hasLinks("a"))
  }
}
