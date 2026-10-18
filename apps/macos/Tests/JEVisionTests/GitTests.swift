import Foundation
import JEVisionCore
import Testing

@testable import JEVision

@MainActor
struct FlowDiffTests {
  private static func flow(_ test: String, _ blocks: String) throws -> Flow {
    try loadFlow(json: #"{"format":1,"#  + test + #","start":"open","blocks":{"# + blocks + "}}")
  }

  /// Every setting of every block, and of the test, changed at once.
  @Test func everyChangedSettingHasAName() throws {
    let before = try Self.flow(
      #""name":"A","app":"com.example.A","tags":[],"retries":0"#,
      #"""
      "open":{"type":"openApp","next":"file"},
      "file":{"type":"openFile","files":["a.txt"],"next":"click"},
      "click":{"type":"click","target":{"image":"refs/a.png"},"button":"left","next":"type"},
      "type":{"type":"type","text":"a","next":"keys"},
      "keys":{"type":"pressKeys","keys":"a","next":"wait"},
      "wait":{"type":"waitFor","until":"screenStable","timeout":"5s","next":"check"},
      "check":{"type":"check","reference":{"image":"refs/a.png"},"next":"loop"},
      "loop":{"type":"repeat","times":2,"body":"inner","next":"data"},
      "inner":{"type":"pressKeys","keys":"a"},
      "data":{"type":"useData","file":"a.csv","body":"row","next":"run"},
      "row":{"type":"pressKeys","keys":"a"},
      "run":{"type":"runTest","test":"a.flow.json","next":"swap"},
      "swap":{"type":"pressKeys","keys":"a"}
      """#)
    let after = try Self.flow(
      #""name":"B","description":"B","app":"com.example.B","testType":"smoke","priority":"high","status":"ready","tags":["b"],"retries":1"#,
      #"""
      "open":{"type":"openApp","app":"com.example.B","next":"file"},
      "file":{"type":"openFile","files":["b.txt"],"app":"com.example.B","newInstance":true,"next":"click"},
      "click":{"type":"click","target":{"image":"refs/b.png"},"button":"right","double":true,"waitForChange":true,"next":"type"},
      "type":{"type":"type","text":"b","into":{"image":"refs/b.png"},"next":"keys"},
      "keys":{"type":"pressKeys","keys":"b","next":"wait"},
      "wait":{"type":"waitFor","until":{"appears":{"image":"refs/b.png"}},"timeout":"6s","next":"check"},
      "check":{"type":"check","reference":{"image":"refs/b.png"},"next":"loop"},
      "loop":{"type":"repeat","times":3,"body":"inner","next":"data"},
      "inner":{"type":"pressKeys","keys":"a"},
      "data":{"type":"useData","file":"b.csv","body":"row","next":"run"},
      "row":{"type":"pressKeys","keys":"a"},
      "run":{"type":"runTest","test":"b.flow.json","next":"swap"},
      "swap":{"type":"type","text":"a"}
      """#)

    let diff = diffFlows(before: before, after: after)
    let fields = Set(diff.blocks.flatMap(\.fields) + diff.testFields)
    #expect(fields.count == 24)
    for field in fields {
      #expect(FlowDiffView.fieldName(field) != field, "“\(field)” is shown by its name in the file")
    }
    #expect(diff.blocks.allSatisfy { $0.kind == .changed })
    #expect(!diff.blocks.contains { ["inner", "row"].contains($0.id) })
  }
}

struct GitTextTests {
  @Test func everyProblemHasItsOwnWords() {
    let errors: [GitError] = [
      .NotInstalled, .NotRepository, .NoCommits, .NoRemote, .NoUpstream(branch: "main"), .Detached, .SignIn,
      .RemoteNotFound, .Unreachable, .Rejected, .Conflict(paths: ["auth/login.flow.json"]),
      .LocalChanges(paths: ["a.flow.json", "b.png"]), .NoIdentity, .EmptyMessage, .NothingToCommit,
      .BadBranchName(name: "a b"), .BranchExists(name: "main"), .NotInCommit(path: "a.flow.json"),
    ]
    let messages = errors.map(GitText.failure)
    #expect(Set(messages).count == errors.count)
    #expect(messages[10].contains("“login”"), "a test is named without its folder and extension")
    #expect(GitText.failure(GitError.Failed(message: "fatal: bad object")) == "fatal: bad object")
  }

  @Test func sortsTestsFromOtherFiles() {
    #expect(GitText.isTest("auth/Sign In.flow.json"))
    #expect(!GitText.isTest("refs/home.png"))
    #expect(GitText.isPicture("refs/Home.PNG"))
    #expect(GitText.fileName("auth/Sign In.flow.json") == "Sign In")
    #expect(GitText.fileName("refs/home.png") == "home.png")
  }
}
