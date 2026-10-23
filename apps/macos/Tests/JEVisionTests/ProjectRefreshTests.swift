import Foundation
import JEVisionCore
import Testing

@testable import JEVision

/// What the project reads again after a change on disk: only what changed.
@MainActor
struct ProjectRefreshTests {
  private func folder() throws -> URL {
    let folder = FileManager.default.temporaryDirectory.appending(path: "refresh-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    return folder
  }

  /// Waits until `done`, looking every 10 ms, for at most ten seconds.
  private func wait(for what: String, until done: () -> Bool) async throws {
    let deadline = ContinuousClock.now + .seconds(10)
    while !done() {
      try #require(ContinuousClock.now < deadline, "Gave up waiting for \(what)")
      try await Task.sleep(for: .milliseconds(10))
    }
  }

  private func test(_ name: String) -> String {
    #"{"format":1,"name":"\#(name)","app":"com.example.App","start":"open","blocks":{"open":{"type":"openApp"}}}"#
  }

  @Test func aRunSavedListsTheRunWithoutLookingThroughTheTestsAgain() async throws {
    let root = try folder().appending(path: "Shop")
    let spec = SyntheticSpec(
      tests: 4, minBlocks: 30, maxBlocks: 40, folders: 2, depth: 1, reports: 3,
      endMs: UInt64(Date.now.timeIntervalSince1970 * 1000))
    let summary = try writeSyntheticProject(dir: root.path, spec: spec)
    let store = ProjectStore()
    store.open(root)
    try await wait(for: "the project to open") { store.status == .ready && store.runs.count == Int(summary.reports) }
    let scans = store.scanCache.scanCount
    let reports = try #require(store.runsFolder).reportCount

    let source = try #require(store.runs.first).dir
    try FileManager.default.copyItem(
      at: URL(filePath: source), to: root.appending(path: ".jevision/runs/29991231-000000-copy"))
    try await wait(for: "the new run to be listed") { store.runs.count == Int(summary.reports) + 1 }

    #expect(store.scanCache.scanCount == scans)
    #expect(try #require(store.runsFolder).reportCount == reports + 1)
    #expect(store.runIndex.run(store.runs.first?.id) != nil)
  }

  @Test func aScanReadsOnlyTheTestsThatChanged() throws {
    let root = try folder()
    try test("Sign in").write(to: root.appending(path: "sign-in.flow.json"), atomically: true, encoding: .utf8)
    try test("Cart").write(to: root.appending(path: "cart.flow.json"), atomically: true, encoding: .utf8)
    let cache = ProjectScanner.Cache()

    let first = try ProjectScanner.scan(root, cache: cache)
    #expect(cache.readCount == 2)
    let again = try ProjectScanner.scan(root, cache: cache)
    #expect(cache.readCount == 2)
    #expect(again.stamp == first.stamp)
    #expect(again.items == first.items)

    try test("Cart and checkout").write(to: root.appending(path: "cart.flow.json"), atomically: true, encoding: .utf8)
    let changed = try ProjectScanner.scan(root, cache: cache)
    #expect(cache.readCount == 3)
    #expect(changed.stamp != first.stamp)
    #expect(changed.items.first { $0.id == "cart.flow.json" }?.keywords.contains("checkout") == true)

    try FileManager.default.moveItem(at: root.appending(path: "cart.flow.json"), to: root.appending(path: "basket.flow.json"))
    try FileManager.default.removeItem(at: root.appending(path: "sign-in.flow.json"))
    let moved = try ProjectScanner.scan(root, cache: cache)
    #expect(moved.items.map(\.id) == ["basket.flow.json"])
    #expect(moved.stamp != changed.stamp)
  }
}
