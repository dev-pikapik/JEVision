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
      #""name":"B","description":"B","app":"com.example.B","testType":"smoke","priority":"high","status":"ready","tags":["b"],"preconditions":"B","author":"B","platform":"B","link":"https://b.example","flaky":true,"retries":1"#,
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
    #expect(fields.count == 29)
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
      .NoGitHubCli, .GitHubSignedOut, .SignInExpired, .SignInDenied, .SignInUnavailable, .NotGitHub, .NoPermission, .NotPushed(branch: "feat/login"),
      .PullRequestExists, .NothingToPropose(base: "main"),
    ]
    let messages = errors.map(GitText.failure)
    #expect(Set(messages).count == errors.count)
    #expect(messages[10].contains("“login”"), "a test is named without its folder and extension")
    #expect(GitText.failure(GitError.Failed(message: "fatal: bad object")) == "fatal: bad object")
    #expect(GitText.failure(GitError.GitHubSignedOut).contains("Settings"), "says where to sign in")
    #expect(GitText.failure(GitError.SignInUnavailable).contains("Client ID"), "says what to check")
    #expect(GitText.failure(GitError.NoGitHubCli).contains("cli.github.com"), "says where to get GitHub CLI")
    #expect(GitText.failure(GitError.NotPushed(branch: "feat/login")).contains("“feat/login”"))
    #expect(GitText.failure(GitError.NothingToPropose(base: "main")).contains("“main”"))
  }

  @Test func sortsTestsFromOtherFiles() {
    #expect(GitText.isTest("auth/Sign In.flow.json"))
    #expect(!GitText.isTest("refs/home.png"))
    #expect(GitText.isPicture("refs/Home.PNG"))
    #expect(GitText.fileName("auth/Sign In.flow.json") == "Sign In")
    #expect(GitText.fileName("refs/home.png") == "home.png")
  }
}

@MainActor
struct GitHubSignInTests {
  @Test func jevisionSignsInThroughItsOwnApp() {
    #expect(ProjectGit.gitHubClientID.wholeMatch(of: /Ov23li[A-Za-z0-9]{14}/) != nil, "a GitHub OAuth app’s Client ID")
  }

  @Test func ownClientIDIsOptional() {
    #expect(ProjectGit.signInClientID(typed: "") == ProjectGit.gitHubClientID)
    #expect(ProjectGit.signInClientID(typed: "  ") == ProjectGit.gitHubClientID)
    #expect(ProjectGit.signInClientID(typed: " Ov23liMine ") == "Ov23liMine")
    #expect(!ProjectGit.signInClientID(typed: "").isEmpty, "signing in never needs a Client ID typed")
  }

  @Test func signingOutPointsToTheAccessOfTheAppSignedInThrough() {
    let page = ProjectGit.SignedOut(clientID: "Ov23liMine").accessPage
    #expect(page?.absoluteString == "https://github.com/settings/connections/applications/Ov23liMine")
    #expect(ProjectGit.SignedOut(clientID: nil).accessPage == nil, "GitHub CLI’s access isn’t JEVision’s to take away")
  }
}

@MainActor
struct ConflictFileTests {
  private static func test(_ timeout: String, tags: String = "[]") -> Data {
    Data(#"""
      {"format":1,"name":"Wait","app":"com.example.A","tags":\#(tags),"start":"open","blocks":{
      "open":{"type":"openApp","next":"wait"},
      "wait":{"type":"waitFor","until":"screenStable","timeout":"\#(timeout)"}}}
      """#.utf8)
  }

  @Test func aTestIsSettledBlockByBlock() throws {
    var file = ConflictFile(
      path: "auth/Wait.flow.json",
      versions: ConflictVersions(base: Self.test("5s"), mine: Self.test("7s"), theirs: Self.test("9s", tags: #"["smoke"]"#)))
    let test = try #require(file.test)
    #expect(test.conflicts.map(\.id) == ["wait"])
    #expect(ConflictText.title(test.conflicts[0], test) != "wait", "the block is named by its kind")
    #expect(ConflictText.value(test.conflicts[0], in: test.mine).contains("7"))

    file.chooseAll(.both)
    #expect(file.choices == [.mine], "a setting can't keep both")
    file.choices = [.theirs]
    let settled = try #require(file.settled)
    let flow = try loadFlow(json: String(decoding: try #require(settled.contents), as: UTF8.self))
    #expect(flow.tags == ["smoke"], "what only the server changed comes in by itself")
    guard case .waitFor(_, let timeoutMs, _) = flow.blocks.first(where: { $0.id == "wait" })?.kind else {
      Issue.record("the wait block is kept")
      return
    }
    #expect(timeoutMs == 9000)
  }

  @Test func otherFilesKeepOneSide() {
    var file = ConflictFile(
      path: "refs/home.png", versions: ConflictVersions(base: Data([1]), mine: nil, theirs: Data([2])))
    #expect(file.test == nil)
    #expect(file.settled?.contents == nil, "mine removed it")
    file.chooseAll(.theirs)
    #expect(file.settled?.contents == Data([2]))
  }
}
