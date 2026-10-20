import JEVisionCore
import Testing

private let smokeTest = """
  {
    "format": 1,
    "name": "New document",
    "app": "com.apple.TextEdit",
    "start": "open",
    "blocks": {
      "open": { "type": "openApp", "next": "new" },
      "new": { "type": "pressKeys", "keys": "cmd+n" }
    }
  }
  """

@Test func loadEditAndSave() throws {
  var flow = try loadFlow(json: smokeTest)
  #expect(flow.app == "com.apple.TextEdit")
  // Nothing in it is checked, which is all that is wrong with it.
  #expect(validateFlow(flow: flow).map(\.code) == ["flow.noCheck"])

  let index = try #require(flow.blocks.firstIndex { $0.id == "new" })
  flow.blocks[index].kind = .pressKeys(keys: "cmd+shift+n", next: nil)
  let saved = saveFlow(flow: flow)
  #expect(saved.hasPrefix("{\n  \"format\": \(formatVersion()),\n"))
  #expect(saved.contains("\"keys\": \"cmd+shift+n\""))
  #expect(try loadFlow(json: saved) == flow)
}

@Test func unreadableTextReportsIssueCodes() {
  do {
    _ = try loadFlow(json: "{")
    Testing.Issue.record("loadFlow accepted invalid JSON")
  } catch let FlowError.Invalid(issues) {
    #expect(issues.map(\.code) == ["flow.json.syntax"])
  } catch {
    Testing.Issue.record("unexpected error \(error)")
  }
}

@Test func validationNamesTheBrokenEdge() throws {
  var flow = try loadFlow(json: smokeTest)
  flow.start = "missing"
  let issue = try #require(validateFlow(flow: flow).first)
  #expect(issue.code == "flow.start.unknownBlock")
  #expect(issue.params == [IssueParam(name: "target", value: "missing")])
}

@Test func catalogListsEveryBlockType() {
  let names = blockCatalog().map(\.typeName)
  #expect(names.count == 20)
  #expect(names.contains("repeat") && names.contains("useData") && names.contains("script")
    && names.contains("scroll") && names.contains("pause"))
}

@Test func scriptsHandValuesToLaterBlocks() throws {
  var flow = try loadFlow(json: smokeTest)
  let index = try #require(flow.blocks.firstIndex { $0.id == "new" })
  flow.blocks[index].kind = .script(
    language: .python3, code: "print(1)", inputs: [NamedValue(name: "NAME", value: "Ann")],
    outputs: [ScriptOutput(name: "total", kind: .number)], timeoutMs: 30_000, next: nil)
  #expect(validateFlow(flow: flow).map(\.code) == ["flow.noCheck"])
  #expect(savedVariables(flow: flow) == ["total"])
  #expect(try loadFlow(json: saveFlow(flow: flow)) == flow)
}
