import Foundation
import JEVisionCore
import Testing

@testable import JEVision

struct ProjectMapTests {
  private func test(_ id: String) -> ProjectItem {
    ProjectItem(
      id: id, name: ProjectScanner.testName((id as NSString).lastPathComponent), url: URL(filePath: "/p/" + id),
      kind: .test, children: [], testCount: 1, modified: nil)
  }

  private func folder(_ id: String, _ children: [ProjectItem]) -> ProjectItem {
    ProjectItem(
      id: id, name: (id as NSString).lastPathComponent, url: URL(filePath: "/p/" + id), kind: .folder,
      children: children, testCount: children.reduce(0) { $0 + $1.testCount }, modified: nil)
  }

  private func mapTest(_ file: String, uses: [String]) -> MapTest {
    MapTest(file: file, name: file, blocks: 1, uses: uses, errors: 0, readable: true)
  }

  @Test func groupsTestsByTheFolderTheyAreIn() {
    let items = [
      folder("smoke", [folder("smoke/deep", [test("smoke/deep/c.flow.json")]), test("smoke/b.flow.json")]),
      folder("empty", [folder("empty/only", [test("empty/only/d.flow.json")])]),
      test("a.flow.json"),
    ]
    let groups = ProjectMap.groups(items, rootTitle: "Acme")
    #expect(groups.map(\.title) == ["Acme", "smoke", "smoke/deep", "empty/only"])
    #expect(groups.map { $0.tests.map(\.id) } == [
      ["a.flow.json"], ["smoke/b.flow.json"], ["smoke/deep/c.flow.json"], ["empty/only/d.flow.json"],
    ])
    #expect(items.flatMap(\.tests).map(\.id) == [
      "smoke/deep/c.flow.json", "smoke/b.flow.json", "empty/only/d.flow.json", "a.flow.json",
    ])
  }

  @Test func linksATestToTheTestsItRuns() {
    let map = [
      "a.flow.json": mapTest("a.flow.json", uses: ["shared/login.flow.json", "a.flow.json", "gone.flow.json"]),
      "b.flow.json": mapTest("b.flow.json", uses: ["shared/login.flow.json"]),
      "shared/login.flow.json": mapTest("shared/login.flow.json", uses: []),
    ]
    #expect(ProjectMap.usedBy(map) == ["shared/login.flow.json": ["a.flow.json", "b.flow.json"], "gone.flow.json": ["a.flow.json"]])
    let a = CGRect(x: 0, y: 0, width: 10, height: 10)
    let login = CGRect(x: 50, y: 0, width: 10, height: 10)
    let links = ProjectMap.links(
      map, bounds: ["a.flow.json": a, "shared/login.flow.json": login])
    // Itself, a missing test and a test not on screen draw nothing.
    #expect(links == [ProjectMap.Link(from: a, to: login)])
  }

  @MainActor @Test func runAllCountsTestsThatCannotStart() {
    let center = RunCenter(defaults: UserDefaults(suiteName: "ProjectMapTests")!)
    let missing = [URL(filePath: "/nonexistent/a.flow.json"), URL(filePath: "/nonexistent/b.flow.json")]
    center.runAll(missing, in: URL(filePath: "/nonexistent"), title: "Smoke")
    #expect(!center.isRunning)
    #expect(center.batch == nil)
    let result = RunCenter.BatchResult(title: "Smoke", total: 2, passed: 0, failed: 0, stopped: false)
    #expect(center.batchResult == result)
    #expect(RunText.batchOutcome(result) == "Smoke finished. Passed: 0 · Failed: 0 · Not run: 2")
    center.dismissBatchResult()
    #expect(center.batchResult == nil)
  }
}
