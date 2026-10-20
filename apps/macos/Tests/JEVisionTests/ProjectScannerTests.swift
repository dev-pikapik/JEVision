import Foundation
import Testing

@testable import JEVision

struct ProjectScannerTests {
  @Test func recognizesTestFiles() {
    #expect(ProjectScanner.isTestFile("sign-in.flow.json"))
    #expect(ProjectScanner.isTestFile("Sign-In.FLOW.JSON"))
    #expect(!ProjectScanner.isTestFile(".flow.json"))
    #expect(!ProjectScanner.isTestFile("sign-in.json"))
    #expect(!ProjectScanner.isTestFile("sign-in.flow.json.bak"))
    #expect(ProjectScanner.testName("sign-in.flow.json") == "sign-in")
    #expect(ProjectScanner.testName("Sign-In.FLOW.JSON") == "Sign-In")
  }

  @Test func scansOnlyFoldersWithTests() throws {
    let root = try makeProject([
      "a10.flow.json", "a2.flow.json", "notes.md", ".hidden.flow.json",
      "Smoke/login.flow.json", "Smoke/Deep/checkout.flow.json",
      "Empty/readme.txt", ".git/hook.flow.json", "Bundle.app/inside.flow.json",
    ])
    defer { try? FileManager.default.removeItem(at: root) }
    try FileManager.default.createSymbolicLink(
      at: root.appending(path: "Link"), withDestinationURL: root.appending(path: "Smoke"))

    let items = try ProjectScanner.scan(root)

    #expect(items.map(\.name) == ["Smoke", "a2", "a10"])
    let smoke = try #require(items.first)
    #expect(smoke.kind == .folder)
    #expect(smoke.testCount == 2)
    #expect(smoke.children.map(\.id) == ["Smoke/Deep", "Smoke/login.flow.json"])
    #expect(items.item(withID: "Smoke/Deep/checkout.flow.json")?.parentPath == "Smoke/Deep")
    #expect(items[1].outlineChildren == nil)
  }

  @Test func searchFindsTestsAtAnyDepth() throws {
    let root = try makeProject(["Sign-up.flow.json", "Smoke/sign-in.flow.json", "Smoke/checkout.flow.json"])
    defer { try? FileManager.default.removeItem(at: root) }
    let items = try ProjectScanner.scan(root)

    #expect(ProjectScanner.tests(in: items, matching: " SIGN ").map(\.id) == [
      "Smoke/sign-in.flow.json", "Sign-up.flow.json",
    ])
    #expect(ProjectScanner.tests(in: items, matching: "missing").isEmpty)
  }

  @Test func searchFindsTestsByTheirNameAndWhatTheyDo() throws {
    let root = try makeProject(["flow-012.flow.json", "flow-013.flow.json"])
    defer { try? FileManager.default.removeItem(at: root) }
    try Data(
      #"""
      {"format":1,"name":"Add a line to a note 012","description":"Keeps the draft","app":"com.apple.Notes",
       "author":"Anna Lee","flaky":true,"start":"a","blocks":{"a":{"type":"type","text":"Shopping list"}}}
      """#.utf8
    ).write(to: root.appending(path: "flow-012.flow.json"))
    let items = try ProjectScanner.scan(root)

    #expect(ProjectScanner.tests(in: items, matching: "note 01").map(\.id) == ["flow-012.flow.json"])
    #expect(ProjectScanner.tests(in: items, matching: "draft").map(\.id) == ["flow-012.flow.json"])
    #expect(ProjectScanner.tests(in: items, matching: "shopping").map(\.id) == ["flow-012.flow.json"])
    #expect(ProjectScanner.tests(in: items, matching: "anna").map(\.id) == ["flow-012.flow.json"])
    #expect(items.filter(\.markedFlaky).map(\.id) == ["flow-012.flow.json"], "the flaky mark is read with the words")
    #expect(ProjectScanner.tests(in: items, matching: "flow-01").count == 2)
  }

  @Test func throwsForMissingFolder() {
    #expect(throws: (any Error).self) {
      try ProjectScanner.scan(URL(filePath: "/nonexistent-\(UUID().uuidString)"))
    }
  }

  private func makeProject(_ paths: [String]) throws -> URL {
    let root = FileManager.default.temporaryDirectory.appending(path: "JEVisionTests-\(UUID().uuidString)")
    for path in paths {
      let file = root.appending(path: path)
      try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
      try Data("{}".utf8).write(to: file)
    }
    return root
  }
}
