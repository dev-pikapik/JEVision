import AppKit
import Foundation
import JEVisionCore
import JEVisionHands
import Testing

@testable import JEVision

struct BlockSummaryTests {
  private func summary(_ kind: BlockKind) -> String {
    BlockSummary.text(kind, appName: { $0 == "com.apple.Mail" ? "Mail" : $0 })
  }

  @Test func describesWhatABlockDoes() {
    #expect(summary(.openApp(app: "com.apple.Mail", resetState: false, windowSize: nil, windowPosition: nil, arguments: [], environment: [:], next: nil)) == "Mail")
    #expect(summary(.openApp(app: nil, resetState: false, windowSize: nil, windowPosition: nil, arguments: [], environment: [:], next: nil)) == "The app under test")
    #expect(
      summary(.type(text: "${secret.ACME_PASSWORD}", into: .image("refs/password-field.png"), next: nil))
        == "Type secret ACME_PASSWORD into “password field”")
    #expect(summary(.type(text: "alice@example.com", into: nil, next: nil)) == "Type “alice@example.com”")
    #expect(
      summary(.click(target: .image("refs/sign_in-button.png"), button: .left, double: false, modifiers: .none, waitForChange: false, next: nil))
        == "Click “sign in button”")
    #expect(
      summary(.click(target: .image(""), button: .left, double: false, modifiers: .none, waitForChange: false, next: nil))
        == "Choose what to click")
    #expect(summary(.pressKeys(keys: "cmd+shift+s", next: nil)) == "Press ⇧⌘S")
    #expect(summary(.openFile(files: ["docs/a.txt", "b.txt", "c.txt"], app: nil, newInstance: false, closeAfter: false, next: nil)) == "a.txt and 2 more")
    #expect(summary(.runTest(test: "shared/sign-in.flow.json", package: nil, inputs: [:], next: nil)) == "Runs “sign-in”")
  }

  @Test func neverSplitsADataToken() {
    let shown = summary(.type(text: "Hello, ${data.name}!", into: nil, next: nil))
    #expect(shown == "Type “Hello, " + unbreakablePlaceholders(text: "${data.name}") + "!”")
    #expect(shown.replacing("\u{2060}", with: "") == "Type “Hello, ${data.name}!”")
    #expect(BlockSummary.quoted("Dear customer, ${data.full_name}") == "“Dear customer,…”", "Cut before the token")
  }

  @Test func namesEveryWayOutOfAnIf() {
    let plain = BlockKind.if(
      condition: .screen(
        Reference(image: "a.png", compare: .exact, area: nil, ignore: [], hide: [])), then: nil,
      elseIf: [], otherwise: nil)
    #expect(summary(plain) == "If “a” is on screen")
    #expect(plain.branchName("then") == "Yes")
    #expect(plain.branchName("else") == "No")
    #expect(plain.branchName("elseIf1") == nil)
    let three = addIfCondition(kind: addIfCondition(kind: plain))
    #expect(summary(three) == "If “a” is on screen · or 2 more")
    #expect(
      ["then", "elseIf1", "elseIf2", "else"].map { three.branchName($0) } == [
        "Condition 1", "Condition 2", "Condition 3", "Otherwise",
      ])
    #expect(three.branchName("elseIf3") == nil)
    #expect(three.branchName("next") == nil)
    #expect(
      BlockKind.check(
        of: .screen(Reference(image: "", compare: .exact, area: nil, ignore: [], hide: [])),
        next: nil
      ).branchName("then") == nil)
  }

  @Test func describesBlocksThatDontLookAtTheScreen() {
    #expect(
      summary(.script(language: .python3, code: "\n  print(1)\nprint(2)", inputs: [], outputs: [ScriptOutput(name: "total", kind: .number)], timeoutMs: 1000, next: nil))
        == "Python 3: “print(1)” → total")
    #expect(summary(.script(language: .sh, code: " \n", inputs: [], outputs: [], timeoutMs: 1000, next: nil)) == "No code yet")
    #expect(
      summary(.command(command: "ls", folder: nil, exitCode: 0, saveOutput: "listing", timeoutMs: 1000, next: nil))
        == "“ls” → listing")
    #expect(
      summary(.http(method: .post, url: "https://example.com/items", headers: [], body: "", expectStatus: .success,
        save: [SavedValue(name: "id", from: "body.id")], timeoutMs: 1000, next: nil))
        == "POST https://example.com/items → id")
    #expect(
      summary(.checkFile(path: "~/Downloads/report.pdf", until: .contains("ok"), timeoutMs: 5000, next: nil))
        == "Until “report.pdf” contains “ok” · " + BlockSummary.duration(5000))
    #expect(
      summary(.database(file: "app.sqlite", query: "select count(*) from notes", saveAs: "rows", expect: nil, timeoutMs: 1000, next: nil))
        == "“app.sqlite”: “select count(*) from notes” → rows")
    let value = BlockKind.if(
      condition: .value(ValueCheck(of: "${var.total}", is: .greaterThan, to: "3")), then: nil, elseIf: [], otherwise: nil)
    #expect(summary(value) == unbreakablePlaceholders(text: "If ${var.total} > “3”"))
    for kind in BlockInfo.catalog.map(\.typeName) {
      #expect(BlockInfo.symbol(kind) != "square.dashed", "\(kind) has no symbol")
      #expect(BlockInfo.title(kind) != kind || kind == "if", "\(kind) has no title")
      #expect(!BlockInfo.tagline(kind).isEmpty, "\(kind) has no tagline")
    }
  }

  @Test func shortensLongText() {
    #expect(BlockSummary.quoted("short") == "“short”")
    #expect(BlockSummary.quoted("- milk\n\tok\r\n") == "“- milk⏎⇥ok⏎”")
    #expect(BlockSummary.quoted(String(repeating: "a", count: 30)) == "“" + String(repeating: "a", count: 23) + "…”")
    #expect(BlockSummary.secretName("${secret.X_1}") == "X_1")
    #expect(BlockSummary.secretName("pass ${secret.X}") == nil)
  }

  @Test func describesPointerBlocks() {
    #expect(summary(.hover(target: .image(""), modifiers: .none, waitForChange: false, next: nil)) == "Choose what to point at")
    #expect(summary(.hover(target: .image("refs/help-icon.png"), modifiers: .none, waitForChange: false, next: nil)) == "Point at “help icon”")
    #expect(summary(.drag(from: .image("refs/file.png"), to: .image(""), button: .left, modifiers: .none, waitForChange: false, next: nil)) == "Choose what to drag and where")
    #expect(
      summary(.drag(from: .image("refs/file.png"), to: .image("refs/trash_bin.png"), button: .left, modifiers: .none, waitForChange: false, next: nil))
        == "Drag “file” to “trash bin”")
    #expect(summary(.scroll(direction: .down, amount: 5, unit: .lines, at: nil, modifiers: .none, waitForChange: false, next: nil)) == "Scroll down 5 lines")
    #expect(summary(.scroll(direction: .up, amount: 3, unit: .lines, at: .image(""), modifiers: .none, waitForChange: false, next: nil)) == "Scroll up 3 lines")
    #expect(
      summary(.scroll(direction: .right, amount: 240, unit: .pixels, at: .image("refs/list.png"), modifiers: .none, waitForChange: false, next: nil))
        == "Scroll right 240 px over “list”")
    let option = Modifiers(command: false, shift: false, option: true, control: false)
    let command = Modifiers(command: true, shift: false, option: false, control: false)
    #expect(
      summary(.hover(target: .image("refs/help-icon.png"), modifiers: option, waitForChange: false, next: nil))
        == "Point at “help icon” holding ⌥")
    #expect(
      summary(.scroll(direction: .up, amount: 2, unit: .lines, at: nil, modifiers: command, waitForChange: false, next: nil))
        == "Scroll up 2 lines holding ⌘")
    #expect(
      summary(.scroll(direction: .down, amount: 1, unit: .lines, at: .image("refs/map.png"), modifiers: command, waitForChange: false, next: nil))
        == "Scroll down 1 lines over “map” holding ⌘")
  }

  @Test func namesAreasAndSavedPictures() {
    let area = Target.area("0.1,0.2,0.5,0.3")
    #expect(
      summary(.click(target: area, button: .left, double: false, modifiers: .none, waitForChange: false, next: nil))
        == "Click area “0.1,0.2,0.5,0.3”")
    #expect(summary(.click(target: .area(" "), button: .left, double: false, modifiers: .none, waitForChange: false, next: nil)) == "Choose what to click")
    #expect(
      summary(.hover(target: .area("${var.box}"), modifiers: .none, waitForChange: false, next: nil))
        == unbreakablePlaceholders(text: "Point at area “${var.box}”"))
    #expect(
      summary(.drag(from: area, to: .image("refs/trash_bin.png"), button: .left, modifiers: .none, waitForChange: false, next: nil))
        == "Drag “0.1,0.2,0.5,0.3” to “trash bin”")
    #expect(summary(.type(text: "hi", into: area, next: nil)) == "Type “hi” into area “0.1,0.2,0.5,0.3”")
    #expect(
      summary(.click(target: .image("${var.shot}"), button: .left, double: false, modifiers: .none, waitForChange: false, next: nil))
        == unbreakablePlaceholders(text: "Click “${var.shot}”"))
  }

  @Test func describesKeysHeldAndButtonOfClickAndDrag() {
    let held = Modifiers(command: true, shift: true, option: false, control: false)
    #expect(
      summary(.click(target: .image("refs/row.png"), button: .left, double: false, modifiers: held, waitForChange: false, next: nil))
        == "Click “row” holding ⇧⌘")
    #expect(
      summary(.click(target: .image("refs/row.png"), button: .right, double: false, modifiers: held, waitForChange: false, next: nil))
        == "Right-click “row” holding ⇧⌘")
    #expect(
      summary(
        .drag(from: .image("refs/file.png"), to: .image("refs/trash_bin.png"), button: .right, modifiers: .none, waitForChange: false, next: nil)
      ) == "Right-drag “file” to “trash bin”")
    #expect(
      summary(
        .drag(from: .image("refs/file.png"), to: .image("refs/trash_bin.png"), button: .left, modifiers: held, waitForChange: false, next: nil)
      ) == "Drag “file” to “trash bin” holding ⇧⌘")
    #expect(
      summary(.click(target: .image("refs/row.png"), button: .middle, double: false, modifiers: .none, waitForChange: false, next: nil))
        == "Middle-click “row”")
    #expect(
      summary(
        .drag(from: .image("refs/file.png"), to: .image("refs/trash_bin.png"), button: .middle, modifiers: held, waitForChange: false, next: nil)
      ) == "Middle-drag “file” to “trash bin” holding ⇧⌘")
  }

  @Test func describesClipboardAndDateTimeBlocks() {
    #expect(summary(.clipboard(action: .put(text: ""), next: nil)) == "No text yet")
    #expect(summary(.clipboard(action: .put(text: "hello"), next: nil)) == "Put “hello” on the clipboard")
    #expect(summary(.clipboard(action: .read(saveAs: "copied", content: .text), next: nil)) == "Read the clipboard → copied")
    #expect(summary(.clipboard(action: .read(saveAs: "", content: .image), next: nil)) == "Choose a name to save it as")
    #expect(summary(.clipboard(action: .read(saveAs: "shot", content: .image), next: nil)) == "Read the picture on the clipboard → shot")
    #expect(summary(.clipboard(action: .read(saveAs: "files", content: .files), next: nil)) == "Read the copied files → files")
    #expect(summary(.clipboard(action: .putImage(image: ""), next: nil)) == "No picture yet")
    #expect(summary(.clipboard(action: .putImage(image: "refs/app_logo.png"), next: nil)) == "Put “app logo” on the clipboard")
    #expect(
      summary(.clipboard(action: .check(is: .contains, to: "ok", timeoutMs: 5000), next: nil))
        == "Until the clipboard ∋ “ok” · \(BlockSummary.duration(5000))")
    #expect(summary(.dateTime(saveAs: "", format: "%Y", language: nil, timeZone: nil, shift: nil, next: nil)) == "Choose a name to save it as")
    #expect(summary(.dateTime(saveAs: "today", format: "%Y-%m-%d", language: nil, timeZone: nil, shift: nil, next: nil)) == "“%Y-%m-%d” → today")
    #expect(
      summary(.dateTime(saveAs: "due", format: "%Y-%m-%d", language: nil, timeZone: nil, shift: TimeShift(amount: 3, unit: .days), next: nil))
        == "“%Y-%m-%d” · days +3 → due")
    #expect(
      summary(.dateTime(saveAs: "old", format: "%H:%M", language: nil, timeZone: nil, shift: TimeShift(amount: -1, unit: .hours), next: nil)) == "“%H:%M” · hours -1 → old")
    #expect(
      summary(.dateTime(saveAs: "due", format: "%d %B", language: "ru", timeZone: "Europe/Berlin", shift: nil, next: nil))
        == "“%d %B” · Русский · Europe/Berlin → due")
  }

  @Test func describesAskBlocks() {
    #expect(summary(.ask(question: .isShown(expect: "", provider: "", model: ""), saveAs: "dark", next: nil)) == "No question yet")
    #expect(
      summary(.ask(question: .isShown(expect: "The dark theme is on", provider: "jev", model: ""), saveAs: "dark", next: nil))
        == "On the screen: “The dark theme is on”? → dark")
    #expect(summary(.ask(question: .find(description: ""), saveAs: "send", next: nil)) == "No element yet")
    #expect(summary(.ask(question: .find(description: "the Send button"), saveAs: "", next: nil)) == "Where is “the Send button”?")
    #expect(
      summary(.ask(question: .find(description: "the Send button"), saveAs: "send", next: nil)) == "Where is “the Send button”? → send")
  }

  @Test func namesElementsDescribedInWords() {
    let save = Target.description("the blue Save button")
    let blank = Target.description(" \n")
    #expect(
      summary(.click(target: save, button: .right, double: false, modifiers: .none, waitForChange: false, next: nil))
        == "Right-click “the blue Save button”")
    #expect(summary(.click(target: blank, button: .left, double: false, modifiers: .none, waitForChange: false, next: nil)) == "Choose what to click")
    #expect(summary(.hover(target: save, modifiers: .none, waitForChange: false, next: nil)) == "Point at “the blue Save button”")
    #expect(summary(.hover(target: blank, modifiers: .none, waitForChange: false, next: nil)) == "Choose what to point at")
    #expect(
      summary(.drag(from: .description("the report"), to: .image("refs/trash_bin.png"), button: .left, modifiers: .none, waitForChange: false, next: nil))
        == "Drag “the report” to “trash bin”")
    #expect(
      summary(.scroll(direction: .down, amount: 2, unit: .lines, at: save, modifiers: .none, waitForChange: false, next: nil))
        == "Scroll down 2 lines over “the blue Save button”")
    #expect(summary(.scroll(direction: .down, amount: 2, unit: .lines, at: blank, modifiers: .none, waitForChange: false, next: nil)) == "Scroll down 2 lines")
    #expect(
      summary(.type(text: "hello", into: .description("the search field"), next: nil))
        == "Type “hello” into “the search field”")
    #expect(summary(.type(text: "hello", into: blank, next: nil)) == "Type “hello”")
  }
}

/// The pointer blocks are in the editor like any other: named, drawn, found, and their problems
/// shown next to the setting they are about.
@MainActor
struct PointerBlockTests {
  @Test func everyBlockHasANameASymbolAndWords() {
    for spec in BlockInfo.catalog {
      #expect(BlockInfo.title(spec.typeName) != spec.typeName, "\(spec.typeName) has a title")
      #expect(NSImage(systemSymbolName: BlockInfo.symbol(spec.typeName), accessibilityDescription: nil) != nil)
      #expect(BlockInfo.symbol(spec.typeName) != "square.dashed", "\(spec.typeName) has its own symbol")
      #expect(!BlockInfo.explanation(spec.typeName).isEmpty && !BlockInfo.tagline(spec.typeName).isEmpty)
    }
    let actions = BlockInfo.catalog.filter { $0.group == .actions }.map(\.typeName)
    #expect(["hover", "drag", "scroll"].allSatisfy(actions.contains))
    for type in ["hover", "drag", "scroll"] {
      let flow = try! loadFlow(json: #"{"format":1,"name":"T","app":"com.apple.TextEdit","start":"a","blocks":{"a":{"type":"pressKeys","keys":"a"}}}"#)
      let kind = insertBlock(flow: flow, typeName: type, slot: nil)?.flow.blocks.first { $0.id != "a" }?.kind
      #expect(kind?.typeName == type)
    }
  }

  @Test func paletteFindsThemByWhatTheyDo() {
    #expect(PaletteChoice.search("hover").first == .block("hover"))
    #expect(PaletteChoice.search("tooltip").first == .block("hover"))
    #expect(PaletteChoice.search("drag").first == .block("drag"))
    #expect(PaletteChoice.search("drop").first == .block("drag"))
    #expect(PaletteChoice.search("scroll").first == .block("scroll"))
    #expect(PaletteChoice.search("wheel").first == .block("scroll"))
  }

  @Test func problemsShowNextToTheirSetting() throws {
    let flow = try loadFlow(
      json: #"""
        {"format":1,"name":"T","app":"com.apple.TextEdit","start":"o","blocks":{
          "o":{"type":"openApp","windowSize":{"width":50,"height":600},"windowPosition":{"x":90000,"y":10},"next":"h"},
          "h":{"type":"hover","target":{"point":{"x":2,"y":0.5}},"next":"d"},
          "d":{"type":"drag","from":{"image":"refs/a.png"},"to":{"image":"refs/a.png"},"next":"s"},
          "s":{"type":"scroll","direction":"down","amount":0,"at":{"image":"/abs.png"}}}}
        """#)
    let issues = validateFlow(flow: flow)
    for id in ["o", "h", "d", "s"] {
      let kind = try #require(flow.blocks.first { $0.id == id }?.kind)
      let fields = InspectorView.fields(of: kind)
      let mine = issues.filter { $0.param("block") == id }
      #expect(!mine.isEmpty, "\(id) has a problem")
      for issue in mine {
        let field = issue.param("field")?.split(separator: ".").first.map(String.init) ?? ""
        #expect(fields.contains(field), "\(issue.code) on \(field) shows next to its setting")
        #expect(IssueText.fieldLabel(field) != nil, "\(field) has a name in lists of problems")
      }
    }
    #expect(issues.contains { $0.code == "flow.drag.samePlace" && $0.param("field") == "to" })
    #expect(IssueText.message(issues.first { $0.code == "flow.drag.samePlace" }!).contains("drop"))
  }

  @Test func picturesAndAreasOfTheWrongKindShowNextToTheirSetting() throws {
    let flow = try loadFlow(json: Self.savedPicturesAndAreas)
    let issues = validateFlow(flow: flow)
    for id in ["c", "d", "e", "f"] {
      let kind = try #require(flow.blocks.first { $0.id == id }?.kind)
      let mine = issues.filter { $0.param("block") == id }
      #expect(!mine.isEmpty, "\(id) has a problem")
      for issue in mine {
        let field = issue.param("field")?.split(separator: ".").first.map(String.init) ?? ""
        #expect(InspectorView.fields(of: kind).contains(field), "\(issue.code) on \(field) shows next to its setting")
        #expect(IssueText.fieldLabel(field) != nil, "\(field) has a name in lists of problems")
      }
    }
    let wrong = try #require(issues.first { $0.code == "flow.var.kind" && $0.param("block") == "d" })
    #expect(IssueText.message(wrong) == "This field takes area, but shot is saved as image.")
    let area = try #require(issues.first { $0.code == "flow.target.area" })
    #expect(IssueText.message(area).hasPrefix("“wide” isn’t an area."))
  }

  @Test func offersSavedPicturesAndAreasByTheirKind() throws {
    let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: folder) }
    let url = folder.appending(path: "t.flow.json")
    try Self.savedPicturesAndAreas.write(to: url, atomically: true, encoding: .utf8)
    let context = FieldContext(issues: [], folder: folder, projectRoot: nil, document: FlowDocument(url: url), block: "c")
    #expect(context.variables(of: .image) == ["shot"])
    #expect(context.variables(of: .area) == ["zone"])
    #expect(context.variables(of: .text) == ["files", "said"])
  }

  /// A picture read from the clipboard, an area and a text a script saves, and blocks that use
  /// them in the wrong places.
  private static let savedPicturesAndAreas = #"""
    {"format":1,"name":"T","app":"com.apple.TextEdit","start":"a","blocks":{
      "a":{"type":"clipboard","action":"read","saveAs":"shot","content":"image","next":"a2"},
      "a2":{"type":"clipboard","action":"read","saveAs":"files","content":"files","next":"b"},
      "b":{"type":"script","language":"sh","code":"true","outputs":[{"name":"zone","type":"area"},{"name":"said","type":"text"}],"next":"c"},
      "c":{"type":"click","target":{"image":"${var.zone}"},"next":"d"},
      "d":{"type":"scroll","direction":"up","amount":1,"at":{"area":"${var.shot}"},"next":"e"},
      "e":{"type":"clipboard","action":"put","image":"${var.zone}","next":"f"},
      "f":{"type":"hover","target":{"area":"wide"}}}}
    """#
}

struct KeyComboTests {
  @Test func showsCombinationsTheWayMenusDo() {
    #expect(KeyCombo.display("cmd+shift+s") == "⇧⌘S")
    #expect(KeyCombo.display("ctrl+option+return") == "⌃⌥↩")
    #expect(KeyCombo.display("f5") == "F5")
  }

  @Test func recordsKeysInFileOrder() {
    #expect(KeyCombo.from(keyCode: 1, modifierFlags: [.shift, .command]) == "cmd+shift+s")
    #expect(KeyCombo.from(keyCode: 36, modifierFlags: []) == "return")
    #expect(KeyCombo.from(keyCode: 76, modifierFlags: []) == "return")
    #expect(KeyCombo.from(keyCode: 96, modifierFlags: [.control]) == "ctrl+f5")
    #expect(KeyCombo.from(keyCode: 10, modifierFlags: []) == nil)
  }

  /// On a Russian layout ⌘S types "ы"; the shortcut is still recorded as the key's place.
  @Test func recordsShortcutsTheSameOnEveryLayout() {
    #expect(KeyCombo.from(keyCode: 1, modifierFlags: [.command]) == "cmd+s")
    #expect(KeyCombo.from(keyCode: 45, modifierFlags: [.command]) == "cmd+n")
    #expect(KeyCombo.from(keyCode: 43, modifierFlags: [.command]) == "cmd+,")  }
}

struct RelativePathTests {
  @Test func writesPathsFromTheTestFolder() {
    let tests = URL(filePath: "/p/tests")
    #expect(RelativePath.from(tests, to: URL(filePath: "/p/tests/refs/a.png")) == "refs/a.png")
    #expect(RelativePath.from(tests, to: URL(filePath: "/p/data/users.csv")) == "../data/users.csv")
    #expect(RelativePath.resolve("../data/users.csv", in: tests).path == "/p/data/users.csv")
    #expect(RelativePath.isInside(URL(filePath: "/p/data/users.csv"), folder: URL(filePath: "/p")))
    #expect(!RelativePath.isInside(URL(filePath: "/other/users.csv"), folder: URL(filePath: "/p")))
    #expect(!RelativePath.isInside(URL(filePath: "/pp/users.csv"), folder: URL(filePath: "/p")))
  }

  @Test func findsALabeledFileWhereverItMovesInTheProject() throws {
    let files = FileManager.default
    let project = files.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? files.removeItem(at: project) }
    let tests = project.appending(path: "tests")
    try files.createDirectory(at: project.appending(path: ".jevision"), withIntermediateDirectories: true)
    try files.createDirectory(at: tests.appending(path: "refs"), withIntermediateDirectories: true)
    try files.createDirectory(at: project.appending(path: "pictures"), withIntermediateDirectories: true)
    try Data("picture".utf8).write(to: tests.appending(path: "refs/a.png"))

    let label = FileLabels.reference(to: tests.appending(path: "refs/a.png"), from: tests)
    #expect(label.problem == nil)
    #expect(label.reference.hasPrefix("res:"))
    #expect(RelativePath.path(of: label.reference, in: tests) == "refs/a.png")

    for name in ["a.png", "a.png.jevmeta"] {
      try files.moveItem(at: tests.appending(path: "refs/\(name)"), to: project.appending(path: "pictures/\(name)"))
    }
    #expect(RelativePath.path(of: label.reference, in: tests) == "../pictures/a.png")
    #expect(RelativePath.resolve(label.reference, in: tests).path == project.appending(path: "pictures/a.png").standardizedFileURL.path)
    #expect(RelativePath.path(of: "refs/b.png", in: tests) == "refs/b.png", "a path stays a path")

    let flow = try loadFlow(
      json: #"{"format": 2, "name": "t", "app": "com.example.App", "start": "a", "blocks": {"a": {"type": "check", "reference": {"image": "\#(label.reference)"}}}}"#)
    let root = project.path(percentEncoded: false)
    let plain = plainPaths(project: root, test: "tests/t.flow.json", flow: flow)
    #expect(plain.missing.isEmpty)
    #expect(saveFlow(flow: plain.flow).contains(#""image": "../pictures/a.png""#))
    #expect(FileLabels.fileName(label.reference) == "a.png")
    #expect(BlockSummary.imageName(label.reference) == "“a”")
    #expect(BlockSummary.imageName("res:89abcdef01234567") == "an image", "a label no open project has")

    try files.removeItem(at: project.appending(path: "pictures/a.png"))
    #expect(RelativePath.resolution(label.reference, in: tests)?.status == .missing(name: "a.png"))
    #expect(plainPaths(project: root, test: "tests/t.flow.json", flow: flow).missing == ["a.png"], "a missing file keeps its label")
  }

  @Test func thePreviewFollowsALabeledPictureThatMoved() throws {
    let files = FileManager.default
    let project = files.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? files.removeItem(at: project) }
    try files.createDirectory(at: project.appending(path: ".jevision"), withIntermediateDirectories: true)
    for folder in ["refs", "archive"] {
      try files.createDirectory(at: project.appending(path: folder), withIntermediateDirectories: true)
    }
    for name in ["note.png", "note.dark.png"] { try Data(name.utf8).write(to: project.appending(path: "refs/\(name)")) }
    let label = FileLabels.reference(to: project.appending(path: "refs/note.png"), from: project).reference
    let file = { (path: String) in project.appending(path: path).standardizedFileURL }
    #expect(RelativePath.previewFile(of: label, in: project, dark: false) == file("refs/note.png"))
    #expect(RelativePath.previewFile(of: label, in: project, dark: true) == file("refs/note.dark.png"))

    for name in ["note.png", "note.png.jevmeta", "note.dark.png"] {
      try files.moveItem(at: project.appending(path: "refs/\(name)"), to: project.appending(path: "archive/\(name)"))
    }
    #expect(RelativePath.previewFile(of: label, in: project, dark: false) == file("archive/note.png"))
    #expect(RelativePath.previewFile(of: label, in: project, dark: true) == file("archive/note.dark.png"))
  }

  @Test func picksTheImageForTheAppearanceAsRunsDo() throws {
    let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: folder) }
    try FileManager.default.createDirectory(at: folder.appending(path: "refs"), withIntermediateDirectories: true)
    let dark = folder.appending(path: "refs/x.dark.png")
    try Data().write(to: dark)

    // Only the dark picture: a reference has it, a picture runs need in light mode doesn't.
    #expect(AppearanceImage.url("refs/x.png", in: folder, dark: true) == dark)
    #expect(AppearanceImage.url("refs/x.png", in: folder, dark: false) == nil)
    #expect(AppearanceImage.url("refs/x.png", in: folder, dark: false, variants: true) == dark)

    let light = folder.appending(path: "refs/x.png")
    try Data().write(to: light)
    #expect(AppearanceImage.url("refs/x.png", in: folder, dark: true) == dark)
    #expect(AppearanceImage.url("refs/x.png", in: folder, dark: false, variants: true) == light)
    try FileManager.default.removeItem(at: dark)
    #expect(AppearanceImage.url("refs/x.png", in: folder, dark: true) == light)
    #expect(AppearanceImage.url("", in: folder, dark: true) == nil)
  }

  @Test func namesNewTestFiles() {
    #expect(RelativePath.testFileName(" Sign in: valid / invalid ") == "Sign in- valid - invalid.flow.json")
    #expect(RelativePath.testFileName("  ") == "Untitled.flow.json")
    let taken: Set = ["Login", "Login 2"]
    #expect(RelativePath.unique("Login") { taken.contains($0) } == "Login 3")
    #expect(RelativePath.unique("Logout") { taken.contains($0) } == "Logout")
  }
}

@MainActor
struct FlowDocumentTests {
  private static let chain = """
    {"format": 1, "name": "T", "app": "com.apple.TextEdit", "start": "a", "blocks": {
      "a": {"type": "pressKeys", "keys": "a", "next": "b"},
      "b": {"type": "pressKeys", "keys": "b"}}}
    """

  private func file(_ text: String) throws -> URL {
    let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let url = folder.appending(path: "t.flow.json")
    try text.write(to: url, atomically: true, encoding: .utf8)
    return url
  }

  @Test func keepingPlainPathsTakesOffTheLabelsMadeForTheChangeOnly() throws {
    let files = FileManager.default
    let project = files.temporaryDirectory.appending(path: UUID().uuidString)
    defer {
      FileLabels.setKeepsPlainPaths(false, in: project)
      try? files.removeItem(at: project)
    }
    try files.createDirectory(at: project.appending(path: ".jevision"), withIntermediateDirectories: true)
    try files.createDirectory(at: project.appending(path: "refs"), withIntermediateDirectories: true)
    for name in ["new", "old", "shared"] { try Data(name.utf8).write(to: project.appending(path: "refs/\(name).png")) }
    let root = project.path(percentEncoded: false)
    let old = try labelResource(project: root, file: "refs/old.png")
    let url = project.appending(path: "t.flow.json")
    let test = { (images: [String]) in
      let blocks = images.enumerated().map { index, image in
        let next = index + 1 < images.count ? #", "next": "b\#(index + 1)""# : ""
        return #""b\#(index)": {"type": "click", "target": {"image": "\#(image)"}\#(next)}"#
      }
      return #"{"format": 1, "name": "T", "app": "com.example.App", "start": "b0", "blocks": {\#(blocks.joined(separator: ", "))}}"#
    }
    try test(["refs/new.png"]).write(to: url, atomically: true, encoding: .utf8)
    let document = FlowDocument(url: url, project: project)
    let new = document.reference(to: project.appending(path: "refs/new.png"))
    let shared = document.reference(to: project.appending(path: "refs/shared.png"))
    #expect(document.reference(to: project.appending(path: "refs/old.png")).reference == old)
    try test([shared.reference]).write(to: project.appending(path: "other.flow.json"), atomically: true, encoding: .utf8)

    document.change(try loadFlow(json: test([new.reference, old, shared.reference])), action: "Choose")
    #expect(document.pendingFormat != nil, "the first label of the project waits for the person")
    #expect(files.fileExists(atPath: project.appending(path: "refs/new.png.jevmeta").path))
    #expect(try String(contentsOf: url, encoding: .utf8) == test(["refs/new.png"]))

    document.keepPlainPaths()
    #expect(document.pendingFormat == nil)
    #expect(document.saveError == nil)
    let saved = try String(contentsOf: url, encoding: .utf8)
    #expect(["refs/new.png", "refs/old.png", "refs/shared.png"].allSatisfy { saved.contains(#""image": "\#($0)""#) })
    #expect(!saved.contains("res:"))
    #expect(!files.fileExists(atPath: project.appending(path: "refs/new.png.jevmeta").path), "made for the change")
    #expect(files.fileExists(atPath: project.appending(path: "refs/old.png.jevmeta").path), "it had its label before")
    #expect(files.fileExists(atPath: project.appending(path: "refs/shared.png.jevmeta").path), "another test names it")
  }

  @Test func redoAfterKeepingPlainPathsPutsTheLabelBack() throws {
    let files = FileManager.default
    for keepsPlainPaths in [true, false] {
      let project = files.temporaryDirectory.appending(path: UUID().uuidString)
      defer {
        FileLabels.setKeepsPlainPaths(false, in: project)
        try? files.removeItem(at: project)
      }
      try files.createDirectory(at: project.appending(path: ".jevision"), withIntermediateDirectories: true)
      try files.createDirectory(at: project.appending(path: "refs"), withIntermediateDirectories: true)
      for name in ["a", "b"] { try Data(name.utf8).write(to: project.appending(path: "refs/\(name).png")) }
      let sidecar = project.appending(path: "refs/a.png.jevmeta").path
      let url = project.appending(path: "t.flow.json")
      let test = { (image: String, keys: String) in
        #"{"format": 1, "name": "T", "app": "com.example.App", "start": "b0", "blocks": {"b0": {"type": "click", "target": {"image": "\#(image)"}, "next": "b1"}, "b1": {"type": "pressKeys", "keys": "\#(keys)"}}}"#
      }
      try test("refs/b.png", "a").write(to: url, atomically: true, encoding: .utf8)
      let document = FlowDocument(url: url, project: project)
      let undo = UndoManager()
      undo.groupsByEvent = false
      document.undoManager = undo
      let change = { (flow: Flow) in
        undo.beginUndoGrouping()
        document.change(flow, action: "Change")
        undo.endUndoGrouping()
      }

      let label = document.reference(to: project.appending(path: "refs/a.png")).reference
      change(try loadFlow(json: test(label, "a")))
      change(try loadFlow(json: test(label, "b")))
      undo.undo()
      #expect(document.pendingFormat != nil)
      document.keepPlainPaths()
      #expect(try String(contentsOf: url, encoding: .utf8).contains(#""image": "refs/a.png""#))
      #expect(!files.fileExists(atPath: sidecar), "made for the change")
      FileLabels.setKeepsPlainPaths(keepsPlainPaths, in: project)

      undo.redo()
      #expect(document.saveError == nil, "keeps plain paths: \(keepsPlainPaths)")
      if keepsPlainPaths {
        let saved = try String(contentsOf: url, encoding: .utf8)
        #expect(saved.contains(#""image": "refs/a.png""#) && saved.contains(#""keys": "b""#) && !saved.contains("res:"))
        #expect(!files.fileExists(atPath: sidecar), "taken off again with the plain paths")
      } else {
        #expect(resolveResource(project: project.path(percentEncoded: false), reference: label) == .found(path: "refs/a.png", name: "a.png"))
        #expect(document.pendingFormat != nil, "the question is asked again")
        document.saveWithLabels()
        #expect(document.saveError == nil)
        #expect(try String(contentsOf: url, encoding: .utf8).contains(#""image": "\#(label)""#))
      }
    }
  }

  /// A project with the pictures `refs/a.png` and `refs/b.png` and an open test, with Undo, that
  /// clicks `b`; `test` is the test clicking another picture.
  private func labeledProject() throws -> (project: URL, url: URL, document: FlowDocument, undo: UndoManager, test: (String) -> String) {
    let files = FileManager.default
    let project = files.temporaryDirectory.appending(path: UUID().uuidString)
    try files.createDirectory(at: project.appending(path: ".jevision"), withIntermediateDirectories: true)
    try files.createDirectory(at: project.appending(path: "refs"), withIntermediateDirectories: true)
    for name in ["a", "b"] { try Data(name.utf8).write(to: project.appending(path: "refs/\(name).png")) }
    let url = project.appending(path: "t.flow.json")
    let test = { (image: String) in
      #"{"format": 1, "name": "T", "app": "com.example.App", "start": "b0", "blocks": {"b0": {"type": "click", "target": {"image": "\#(image)"}}}}"#
    }
    try test("refs/b.png").write(to: url, atomically: true, encoding: .utf8)
    let document = FlowDocument(url: url, project: project)
    let undo = UndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    return (project, url, document, undo, test)
  }

  @Test func undoingAPickTakesItsLabelOffUntilRedo() throws {
    let files = FileManager.default
    let (project, url, document, undo, test) = try labeledProject()
    defer {
      FileLabels.setKeepsPlainPaths(false, in: project)
      try? files.removeItem(at: project)
    }
    let sidecar = project.appending(path: "refs/a.png.jevmeta").path

    let label = document.reference(to: project.appending(path: "refs/a.png")).reference
    undo.beginUndoGrouping()
    document.change(try loadFlow(json: test(label)), action: "Choose")
    undo.endUndoGrouping()
    #expect(document.pendingFormat != nil)
    undo.undo()
    #expect(document.pendingFormat == nil)
    #expect(try String(contentsOf: url, encoding: .utf8).contains(#""image": "refs/b.png""#))
    #expect(!files.fileExists(atPath: sidecar), "no test names it")

    undo.redo()
    #expect(resolveResource(project: project.path(percentEncoded: false), reference: label) == .found(path: "refs/a.png", name: "a.png"))
    #expect(document.pendingFormat != nil)
    document.keepPlainPaths()
    #expect(document.saveError == nil)
    #expect(try String(contentsOf: url, encoding: .utf8).contains(#""image": "refs/a.png""#))
    #expect(!files.fileExists(atPath: sidecar), "made for the change")
  }

  @Test func aLabelOfAPictureUndoTrashedGoesOnceRedoBringsItBack() throws {
    let files = FileManager.default
    let (project, url, document, undo, test) = try labeledProject()
    let trash = files.temporaryDirectory.appending(path: UUID().uuidString)
    defer {
      FileLabels.setKeepsPlainPaths(false, in: project)
      try? files.removeItem(at: project)
      try? files.removeItem(at: trash)
    }
    try files.createDirectory(at: trash, withIntermediateDirectories: true)
    let picture = project.appending(path: "refs/a.png")
    let sidecar = project.appending(path: "refs/a.png.jevmeta")

    let label = document.reference(to: picture).reference
    undo.beginUndoGrouping()
    document.change(try loadFlow(json: test(label)), action: "Capture")
    document.trashOnUndo([picture, sidecar]) { file in
      let trashed = trash.appending(path: file.lastPathComponent)
      try files.moveItem(at: file, to: trashed)
      return trashed
    }
    undo.endUndoGrouping()
    undo.undo()
    #expect(!files.fileExists(atPath: picture.path))
    #expect(try String(contentsOf: url, encoding: .utf8).contains(#""image": "refs/b.png""#))

    undo.redo()
    #expect(files.fileExists(atPath: sidecar.path))
    document.keepPlainPaths()
    #expect(try String(contentsOf: url, encoding: .utf8).contains(#""image": "refs/a.png""#))
    #expect(!files.fileExists(atPath: sidecar.path), "made for the change")
  }

  private func nextOf(_ document: FlowDocument, _ id: String) -> String? {
    if case .pressKeys(_, let next) = document.block(id)?.kind { return next }
    if case .click(_, _, _, _, _, let next) = document.block(id)?.kind { return next }
    return nil
  }

  private static func run(of count: Int) -> String {
    let ids = (0..<count).map { "b\($0)" }
    let blocks = ids.enumerated().map { index, id in
      let next = index + 1 < count ? #", "next": "\#(ids[index + 1])""# : ""
      return #""\#(id)": {"type": "pressKeys", "keys": "a"\#(next)}"#
    }
    return #"{"format": 1, "name": "T", "app": "com.apple.TextEdit", "start": "b0", "blocks": {\#(blocks.joined(separator: ", "))}}"#
  }

  @Test func rowsKeepTheStepsAndUndo() throws {
    for count in [1, 3, 4, 7] {
      let document = FlowDocument(url: try file(Self.run(of: count)))
      let undo = UndoManager()
      undo.groupsByEvent = false
      document.undoManager = undo
      let steps = document.order

      undo.beginUndoGrouping()
      document.arrangeInRows()
      undo.endUndoGrouping()
      #expect(document.flow?.canvas.sections.count == (count + 2) / 3, "\(count) blocks")
      #expect(document.order == steps, "step numbers read the same left to right, row by row")
      let rects = document.layout.blocks.map(\.rect)
      #expect(rects.count == count)
      #expect(Set(rects.map(\.y)).count == (count + 2) / 3)

      undo.undo()
      #expect(document.flow?.canvas.sections.isEmpty == true)
      #expect(document.flow?.canvas.positions.isEmpty == true)
    }
  }

  @Test func aRecordingIntoAnEmptyTestIsLaidOutInRows() throws {
    let empty = #"{"format": 1, "name": "T", "app": "", "start": "", "blocks": {}}"#
    let document = FlowDocument(url: try file(empty))
    let keys = (0..<4).map { BlockKind.pressKeys(keys: "\($0)", next: nil) }
    document.insertRecorded(keys, files: [], at: nil, app: "com.apple.TextEdit")
    #expect(document.flow?.blocks.count == 4)
    #expect(document.flow?.canvas.sections.count == 2)

    // Recording more into a test that has blocks leaves its layout alone.
    let other = FlowDocument(url: try file(Self.chain))
    other.insertRecorded(keys, files: [], at: Slot(from: "b", edge: "next"), app: "com.apple.TextEdit")
    #expect(other.flow?.blocks.count == 6)
    #expect(other.flow?.canvas.sections.isEmpty == true)
    #expect(other.flow?.canvas.positions.isEmpty == true)
  }

  @Test func insertsSavesAndUndoes() throws {
    let url = try file(Self.chain)
    let document = FlowDocument(url: url)
    let undo = UndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    #expect(document.order == ["a", "b"])

    undo.beginUndoGrouping()
    document.insert("click", at: Slot(from: "a", edge: "next"))
    undo.endUndoGrouping()
    #expect(document.selection == "click")
    #expect(document.order == ["a", "click", "b"])
    #expect(nextOf(document, "click") == "b")
    #expect(try String(contentsOf: url, encoding: .utf8).contains("\"click\""))

    undo.beginUndoGrouping()
    document.delete("click")
    undo.endUndoGrouping()
    #expect(document.order == ["a", "b"])
    #expect(document.selection == "b")

    undo.undo()
    #expect(document.order == ["a", "click", "b"])
    undo.undo()
    #expect(document.order == ["a", "b"])
    #expect(document.selection == "a" || document.selection == nil)
    undo.redo()
    #expect(document.order == ["a", "click", "b"])
  }

  @Test func typingIntoOneFieldUndoesAsOneStep() throws {
    let document = FlowDocument(url: try file(Self.chain))
    let undo = CountingUndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    document.selection = "a"
    undo.beginUndoGrouping()
    for keys in ["x", "cmd+x", "cmd+shift+x"] {
      document.update("a", field: "keys") { $0 = .pressKeys(keys: keys, next: "b") }
    }
    document.update("b", field: "keys") { $0 = .pressKeys(keys: "y", next: nil) }
    undo.endUndoGrouping()
    #expect(undo.registrations == 2)
    undo.undo()
    #expect(document.block("a")?.kind == .pressKeys(keys: "a", next: "b"))
    #expect(document.block("b")?.kind == .pressKeys(keys: "b", next: nil))
  }

  @Test func movesTheSelectionTogetherAndUndoes() throws {
    let url = try file(Self.chain)
    let document = FlowDocument(url: url)
    let undo = UndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    let before = document.layout.blocks
    document.pick([.block(id: "a"), .block(id: "b")], prefer: "b")
    #expect(document.selection == "b")
    #expect(document.pickedInOrder == [.block(id: "a"), .block(id: "b")])

    undo.beginUndoGrouping()
    document.move(document.pickedInOrder, dx: 165, dy: 0)
    undo.endUndoGrouping()
    let positions = try #require(document.flow?.canvas.positions)
    #expect(positions.count == 2)
    for laid in before {
      let moved = try #require(positions[laid.id])
      #expect(moved.x.truncatingRemainder(dividingBy: FlowCanvas.grid) == 0)
      #expect(abs(moved.x - (laid.rect.x + 165)) <= FlowCanvas.grid)
    }
    #expect(document.picked.count == 2, "moving keeps the selection")
    #expect(try String(contentsOf: url, encoding: .utf8).contains("\"canvas\""))

    undo.undo()
    #expect(document.flow?.canvas.positions.isEmpty == true)
    #expect(document.layout.blocks == before)
  }

  @Test func movingPastTheOriginPersistsAndUndoes() throws {
    let url = try file(Self.chain)
    let document = FlowDocument(url: url)
    let before = document.layout.blocks
    let undo = UndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    undo.beginUndoGrouping()
    document.move([.block(id: "a")], dx: -800, dy: -500, snap: false)
    undo.endUndoGrouping()
    let moved = try #require(document.flow?.canvas.positions["a"])
    #expect(moved.x < 0 && moved.y < 0)
    let reopened = FlowDocument(url: url)
    #expect(reopened.flow?.canvas.positions["a"] == moved)
    #expect(reopened.layout.edges.allSatisfy { !$0.points.isEmpty })
    undo.undo()
    #expect(document.layout.blocks == before)
    undo.redo()
    #expect(document.flow?.canvas.positions["a"] == moved)
  }

  @Test func linksAndUnlinksBlocksButMakesNoLoops() throws {
    let document = FlowDocument(url: try file(Self.chain))
    let undo = UndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    #expect(!document.canConnect("b", edge: "next", to: "a"), "b → a would loop back")
    #expect(!document.canConnect("a", edge: "next", to: "b"), "a already leads to b")
    #expect(!document.canConnect("a", edge: "then", to: "b"), "Press Keys has no Yes branch")

    undo.beginUndoGrouping()
    document.connect("a", edge: "next", to: nil)
    undo.endUndoGrouping()
    #expect(nextOf(document, "a") == nil)
    #expect(document.canConnect("a", edge: "next", to: "b"))
    #expect(document.canConnect("b", edge: "next", to: "a"))
    undo.undo()
    #expect(nextOf(document, "a") == "b")
  }

  @Test func duplicatesAfterTheSelectedBlockAndPastesIntoAnotherTest() throws {
    let document = FlowDocument(url: try file(Self.chain))
    document.undoManager = UndoManager()
    document.pick([.block(id: "a")])
    document.duplicate()
    #expect(document.order == ["a", "a2", "b"])
    #expect(nextOf(document, "a") == "a2")
    #expect(nextOf(document, "a2") == "b")
    #expect(document.selection == "a2")
    #expect(document.picked == [.block(id: "a2")])

    document.pick([.block(id: "a2"), .block(id: "b")])
    let copied = document.copyPicked()
    let other = FlowDocument(url: try file(Self.chain))
    other.pick([])
    #expect(other.paste(copied))
    #expect(other.flow?.blocks.count == 4)
    #expect(other.picked.count == 2)
    #expect(!other.paste("not a test"))
    #expect(other.flow?.blocks.count == 4)
  }

  @Test func addsEditsAndDeletesNotesAndFrames() throws {
    let url = try file(Self.chain)
    let document = FlowDocument(url: url)
    let undo = CountingUndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    undo.beginUndoGrouping()
    document.addNote(at: Point(x: 600, y: 40), author: "Anna")
    for text in ["C", "Ch", "Check twice"] {
      let id = try #require(document.flow?.canvas.notes.first?.id)
      document.editNote(id, text: text)
    }
    undo.endUndoGrouping()
    #expect(undo.registrations == 2, "adding is one step, typing another")
    let note = try #require(document.flow?.canvas.notes.first)
    #expect(note.text == "Check twice")
    #expect(note.author == "Anna")
    #expect(!note.time.isEmpty)
    #expect(document.picked == [.note(id: note.id)])
    #expect(document.selection == nil)

    undo.beginUndoGrouping()
    document.addFrame(Rect(x: 0, y: 0, width: 400, height: 300))
    let frame = try #require(document.flow?.canvas.sections.first)
    document.editFrame(frame.id, title: "Sign in")
    document.resize(.section(id: frame.id), to: Rect(x: 0, y: 0, width: 500, height: 300))
    undo.endUndoGrouping()
    #expect(document.flow?.canvas.sections.first?.title == "Sign in")
    #expect(document.flow?.canvas.sections.first?.rect.width == 496)
    #expect(try String(contentsOf: url, encoding: .utf8).contains("Sign in"))

    document.pick([.note(id: note.id), .section(id: frame.id)])
    undo.beginUndoGrouping()
    document.remove(document.pickedInOrder)
    undo.endUndoGrouping()
    #expect(document.flow?.canvas.notes.isEmpty == true)
    #expect(document.flow?.canvas.sections.isEmpty == true)
    #expect(document.picked.isEmpty)
    #expect(document.order == ["a", "b"])
    undo.undo()
    #expect(document.flow?.canvas.notes.count == 1)
    #expect(document.flow?.canvas.sections.count == 1)
  }

  @Test func reloadsChangesMadeElsewhereKeepingTheSelection() throws {
    let url = try file(Self.chain)
    let document = FlowDocument(url: url)
    document.selection = "b"
    try Self.chain.replacing("\"keys\": \"a\"", with: "\"keys\": \"z\"").write(to: url, atomically: true, encoding: .utf8)
    document.reload()
    #expect(document.block("a")?.kind == .pressKeys(keys: "z", next: "b"))
    #expect(document.selection == "b")

    try "{ broken".write(to: url, atomically: true, encoding: .utf8)
    document.reload()
    #expect(document.flow == nil)
    #expect(document.failure?.isEmpty == false)
  }
}

@MainActor
struct CanvasKeyboardTests {
  /// Open, then a check: Yes types, No presses keys; both go on to the end.
  private static let branches = """
    {"format": 1, "name": "T", "app": "com.apple.TextEdit", "start": "open", "blocks": {
      "open": {"type": "pressKeys", "keys": "a", "next": "if"},
      "if": {"type": "if", "reference": {"image": "a.png"}, "then": "yes", "else": "no"},
      "yes": {"type": "type", "text": "y", "next": "end"},
      "no": {"type": "pressKeys", "keys": "n", "next": "end"},
      "end": {"type": "pressKeys", "keys": "e"}}}
    """

  private func document() throws -> FlowDocument {
    let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let url = folder.appending(path: "t.flow.json")
    try Self.branches.write(to: url, atomically: true, encoding: .utf8)
    return FlowDocument(url: url)
  }

  @Test func arrowsFollowTheLinks() throws {
    let document = try document()
    func go(_ from: String?, _ direction: CanvasDirection) -> String? {
      CanvasNavigation.next(from: from, toward: direction, in: document.layout, order: document.order)
    }
    #expect(go(nil, .down) == "open")
    #expect(go(nil, .up) == document.order.last)
    #expect(go("open", .down) == "if")
    #expect(go("if", .down) == "yes", "Down takes the Yes branch first")
    #expect(go("yes", .right) == "no")
    #expect(go("no", .left) == "yes")
    #expect(go("no", .up) == "if")
    #expect(go("yes", .down) == "end")
    #expect(go("end", .up) == "no", "Of two steps leading here, the one read last")
    #expect(go("open", .up) == nil)
  }

  @Test func linksFromTheKeyboardAndUndoes() throws {
    let document = try document()
    let undo = UndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    document.selection = "yes"

    undo.beginUndoGrouping()
    document.unlink([Slot(from: "yes", edge: "next")])
    undo.endUndoGrouping()
    #expect(document.block("yes")?.kind == .type(text: "y", into: nil, next: nil))

    #expect(document.startLink())
    #expect(document.linking?.slot == Slot(from: "yes", edge: "next"))
    #expect(document.linking?.targets.contains("if") == false, "A link back to the check would loop")
    #expect(document.linking?.target == "no", "Starts at the nearest block it may lead to")
    document.moveLinkTarget(.down)
    #expect(document.linking?.target == "end")
    #expect(!document.finishLink(at: "open"), "Open leads to the check, so this would loop")
    #expect(document.linking != nil)

    undo.beginUndoGrouping()
    #expect(document.finishLink())
    undo.endUndoGrouping()
    #expect(document.linking == nil)
    #expect(document.block("yes")?.kind == .type(text: "y", into: nil, next: "end"))
    undo.undo()
    #expect(document.block("yes")?.kind == .type(text: "y", into: nil, next: nil))

    document.selection = "if"
    #expect(document.startLink())
    #expect(document.linking?.slot.edge == "then")
    #expect(document.startLink(), "Again: the next edge of the same block")
    #expect(document.linking?.slot.edge == "else")
    document.selection = "open"
    #expect(document.linking == nil, "Selecting another block cancels the link")
  }

  @Test func deletesSelectedLinksAsOneStep() throws {
    let document = try document()
    let undo = UndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    let links: Set = [Slot(from: "yes", edge: "next"), Slot(from: "no", edge: "next")]
    document.pickLinks(links)
    #expect(document.pickedLinks == links)
    #expect(document.selection == nil)

    undo.beginUndoGrouping()
    document.unlink(Array(links))
    undo.endUndoGrouping()
    #expect(document.pickedLinks.isEmpty)
    #expect(!document.layout.edges.contains { $0.to == "end" })
    undo.undo()
    #expect(document.layout.edges.filter { $0.to == "end" }.count == 2)
    #expect(!undo.canUndo)
  }

  @Test func zoomFitsAndKeepsThePointUnderThePointer() {
    #expect(Zoom.clamp(5) == Zoom.range.upperBound)
    #expect(Zoom.clamp(0.01) == Zoom.range.lowerBound)
    #expect(Zoom.fit(CGSize(width: 200, height: 100), in: CGSize(width: 800, height: 600)) == 1, "Never blown up")
    #expect(Zoom.fit(CGSize(width: 1000, height: 300), in: CGSize(width: 548, height: 600)) == 0.5)
    #expect(Zoom.fit(CGSize(width: 100_000, height: 100), in: CGSize(width: 800, height: 600)) == Zoom.range.lowerBound)
    #expect(Zoom.fit(.zero, in: CGSize(width: 800, height: 600)) == 1)

    var camera = FlowViewport(offset: CGPoint(x: 100, y: 50), scale: 1)
    let anchor = CGPoint(x: 200, y: 150)
    let point = camera.world(anchor)
    camera.zoom(to: 2, around: anchor)
    #expect(camera.world(anchor) == point)
    camera.offset = CGPoint(x: 900, y: 700)
    let negative = camera.world(.zero)
    #expect(negative.x < 0 && negative.y < 0)
    camera.zoom(to: 0.5, around: .zero)
    #expect(camera.world(.zero) == negative, "Zoom does not clamp at the world origin")
  }

  @Test func diffsNotesAndFramesOfTheCanvas() throws {
    var before = try loadFlow(json: Self.branches)
    before.canvas.notes = [
      Note(
        id: "n1", text: "Old", author: "Ann", time: "", anchor: .test, attachments: [], replies: [],
        resolved: nil,
        rect: Rect(x: 0, y: 0, width: 200, height: 100))
    ]
    var after = before
    after.canvas.notes[0].text = "New"
    after.canvas.sections = [
      CanvasSection(id: "s1", title: "Sign in", rect: Rect(x: 0, y: 0, width: 300, height: 200))
    ]
    let diff = diffFlows(before: before, after: after)
    #expect(diff.notes == [BlockChange(id: "n1", kind: .changed, fields: ["text"])])
    #expect(diff.sections == [BlockChange(id: "s1", kind: .added, fields: [])])
    #expect(diff.testFields.isEmpty)
    after.canvas.notes[0].resolved = Resolved(by: "Ann", time: "2026-01-01T00:00:00Z")
    after.canvas.notes[0].replies = [Reply(id: "r1", author: "Bo", time: "", text: "ok")]
    #expect(
      diffFlows(before: before, after: after).notes
        == [BlockChange(id: "n1", kind: .changed, fields: ["text", "replies", "resolved"])])
    #expect(
      FlowDiffView.marks(diff.sections, on: .before).isEmpty,
      "An added frame isn’t on the old version")
    #expect(FlowDiffView.marks(diff.notes, on: .before) == ["n1": DiffMark(.changed)])
    for field in [
      "text", "title", "author", "position", "size", "anchor", "attachments", "replies", "resolved",
    ] {
      #expect(FlowDiffView.fieldName(field) != field)
    }
  }
}

/// Counts the changes registered for undo: each one names its action.
private final class CountingUndoManager: UndoManager {
  var registrations = 0

  override func setActionName(_ actionName: String) {
    registrations += 1
    super.setActionName(actionName)
  }
}
