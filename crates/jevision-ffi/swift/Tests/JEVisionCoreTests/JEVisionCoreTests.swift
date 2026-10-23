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
  // A test without labels keeps the oldest format, so every JEVision version opens it.
  #expect(saved.hasPrefix("{\n  \"$schema\": \"https://dev-pikapik.github.io/JEVision/schemas/test.schema.json\",\n  \"format\": 1,\n"))
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
  #expect(names.count == 22)
  #expect(names.contains("repeat") && names.contains("useData") && names.contains("script")
    && names.contains("scroll") && names.contains("pause") && names.contains("clipboard")
    && names.contains("dateTime"))
}

@Test func scriptsHandValuesToLaterBlocks() throws {
  var flow = try loadFlow(json: smokeTest)
  let index = try #require(flow.blocks.firstIndex { $0.id == "new" })
  flow.blocks[index].kind = .script(
    language: .python3, code: "print(1)", inputs: [
      ScriptInput(name: "NAME", kind: .text, value: "Ann"),
      ScriptInput(name: "SHOT", kind: .image, value: "refs/a.png"),
    ],
    outputs: [ScriptOutput(name: "total", kind: .number), ScriptOutput(name: "box", kind: .area)],
    timeoutMs: 30_000, next: nil)
  #expect(validateFlow(flow: flow).map(\.code) == ["flow.noCheck"])
  #expect(savedVariables(flow: flow) == ["box", "total"])
  #expect(try loadFlow(json: saveFlow(flow: flow)) == flow)
}

@Test func clicksHoldKeysAndDragsUseTheRightButton() throws {
  var flow = try loadFlow(json: smokeTest)
  let index = try #require(flow.blocks.firstIndex { $0.id == "new" })
  let held = Modifiers(command: true, shift: false, option: true, control: false)
  flow.blocks[index].kind = .drag(
    from: .point(Point(x: 0.1, y: 0.1)), to: .point(Point(x: 0.9, y: 0.9)), button: .right,
    modifiers: held, waitForChange: false, next: nil)
  let saved = saveFlow(flow: flow)
  #expect(saved.contains("\"button\": \"right\""))
  #expect(saved.contains("\"cmd\"") && saved.contains("\"option\""))
  #expect(try loadFlow(json: saved) == flow)
}

@Test func clipboardAndDateBlocksKeepTheirValues() throws {
  var flow = try loadFlow(json: smokeTest)
  let index = try #require(flow.blocks.firstIndex { $0.id == "new" })
  flow.blocks[index].kind = .clipboard(
    action: .read(saveAs: "copied", content: .text), next: "stamp")
  flow.blocks.append(
    Block(
      id: "stamp",
      kind: .dateTime(
        saveAs: "day", format: "%d.%m.%Y", language: nil, timeZone: nil,
        shift: TimeShift(amount: -1, unit: .weeks), next: nil)))
  #expect(savedVariables(flow: flow) == ["copied", "day"])
  #expect(try loadFlow(json: saveFlow(flow: flow)) == flow)
}

@Test func theDateBlockNamesItsLanguagesAndKnowsZones() {
  #expect(dateLanguages() == ["en", "ru"])
  #expect(timeZoneKnown(name: "Europe/Berlin"))
  #expect(!timeZoneKnown(name: "Mars/Olympus"))
}

@Test func aPictureReadFromTheClipboardKeepsItsContentAndTypeInTheFile() throws {
  var flow = try loadFlow(json: smokeTest)
  let index = try #require(flow.blocks.firstIndex { $0.id == "new" })
  flow.blocks[index].kind = .clipboard(
    action: .read(saveAs: "shot", content: .image), next: nil)
  let saved = saveFlow(flow: flow)
  #expect(saved.contains("\"content\": \"image\""))
  #expect(try loadFlow(json: saved) == flow)
}
