import ApplicationServices
import Foundation
import JEVisionCore
import Testing

@testable import JEVision
@testable import JEVisionHands

struct ElementSummaryTests {
  private let save = ElementQuery(id: "", role: "button", title: "Save", index: nil)
  private let unset = ElementQuery(id: "", role: "", title: "", index: nil)

  private func summary(_ kind: BlockKind) -> String {
    BlockSummary.text(kind, appName: { $0 })
  }

  @Test func blocksSayWhichElementAndWhatItShouldBe() {
    #expect(
      summary(.click(target: .element(save), button: .left, double: false, modifiers: .none, waitForChange: false, next: nil))
        == "Click button “Save”")
    #expect(
      summary(.check(of: .element(ElementCheck(element: save, is: .enabled)), next: nil)) == "Button “Save” — enabled")
    let name = ElementQuery(id: "name", role: "textField", title: "", index: 1)
    #expect(
      summary(.waitFor(until: .element(ElementCheck(element: name, is: .valueEquals("Ann"))), timeoutMs: 10_000, next: nil))
        == "Until text field with id “name” (index 1) — value “Ann” · \(BlockSummary.duration(10_000))")
    let checkbox = ElementQuery(id: "", role: "checkBox", title: "", index: nil)
    #expect(
      summary(.if(condition: .element(ElementCheck(element: checkbox, is: .selected)), then: nil, elseIf: [], otherwise: nil))
        == "If checkbox — selected")
  }

  @Test func anElementWithNothingToFindItByAsksForOne() {
    #expect(
      summary(.click(target: .element(unset), button: .left, double: false, modifiers: .none, waitForChange: false, next: nil))
        == "Choose what to click")
    #expect(summary(.check(of: .element(.new), next: nil)) == "Choose an element to check")
    #expect(summary(.waitFor(until: .element(.new), timeoutMs: 5_000, next: nil)) == "Choose an element to wait for")
  }
}

struct ElementEditorTests {
  @Test func everyStateKeepsItsTextWhenAnotherIsChosen() {
    for choice in ElementState.Choice.allCases {
      let state = ElementState(choice, text: "Ann")
      #expect(state.choice == choice)
      let comparesText = [.valueEquals, .valueContains, .titleEquals, .titleContains].contains(choice)
      #expect(state.text == (comparesText ? "Ann" : nil))
    }
    // The editor carries the text over, so switching “value is” to “title is” keeps what was typed.
    let typed = ElementState.valueEquals("Ann")
    #expect(ElementState(.titleEquals, text: typed.text ?? "") == .titleEquals("Ann"))
  }

  @Test func conditionsAreEditedInTheFieldTheyAreWrittenIn() {
    #expect(Condition.element(.new).field == "element")
    #expect(Condition.screen(.new).field == "reference")
    #expect(Condition.value(ValueCheck(of: "", is: .equals, to: "")).field == "value")
    #expect(!CheckOf.element(.new).isScreen)
  }

  @Test func environmentRowsKeepEachNameOnceAndSkipEmptyOnes() {
    #expect(
      EnvironmentEditor.rows(of: ["B": "2", "A": "1"])
        == [NamedValue(name: "A", value: "1"), NamedValue(name: "B", value: "2")])
    let rows = [
      NamedValue(name: "API_URL", value: "${env.API_URL}"), NamedValue(name: "", value: "half typed"),
      NamedValue(name: "API_URL", value: "second"),
    ]
    #expect(EnvironmentEditor.environment(of: rows) == ["API_URL": "${env.API_URL}"])
  }
}

struct WindowElementsTests {
  private struct Fake {
    var raw: RawElement
    var children: [Fake] = []
  }

  private let window = CGRect(x: 100, y: 200, width: 400, height: 200)

  private func flatten(_ roots: [Fake], limit: Int = 100, depth: Int = 64) -> [ElementNode] {
    Accessibility.flatten(roots, window: window, limit: limit, depth: depth) { ($0.raw, $0.children) }
  }

  private var tree: Fake {
    Fake(
      raw: RawElement(role: "AXWindow", title: "Form", frame: window),
      children: [
        // A group that doesn't say where it is: left out, though what is inside it is not.
        Fake(
          raw: RawElement(role: "AXGroup"),
          children: [
            Fake(raw: RawElement(role: "AXButton", id: "save", title: "Save", enabled: false, frame: CGRect(x: 300, y: 250, width: 40, height: 20)))
          ]),
        Fake(
          raw: RawElement(
            role: "AXTextField", subrole: kAXSecureTextFieldSubrole, value: "hunter2", frame: CGRect(x: 100, y: 300, width: 200, height: 20))),
        Fake(raw: RawElement(role: "AXCheckBox", value: "1", frame: CGRect(x: 100, y: 380, width: 20, height: 20))),
      ])
  }

  @Test func elementsComeEachBeforeWhatIsInsideIt() {
    let found = flatten([tree])
    #expect(found.map(\.role) == ["AXWindow", "AXButton", "AXTextField", "AXCheckBox"])
    #expect(found[1].id == "save" && found[1].title == "Save" && !found[1].enabled)
    #expect(found[3].value == "1")
  }

  @Test func framesAreInFractionsOfTheWindow() {
    let found = flatten([tree])
    #expect(found[0].frame == Rect(x: 0, y: 0, width: 1, height: 1))
    #expect(found[1].frame == Rect(x: 0.5, y: 0.25, width: 0.1, height: 0.1))
  }

  @Test func aPasswordNeverLeavesTheApp() {
    let field = flatten([tree]).first { $0.role == "AXTextField" }
    #expect(field?.value == "")
  }

  @Test func theWalkStopsAtItsLimits() {
    #expect(flatten([tree], limit: 2).map(\.role) == ["AXWindow", "AXButton"])
    #expect(flatten([tree], depth: 0).map(\.role) == ["AXWindow"])
    // An element that can't be read is left out with everything inside it.
    let unreadable = Accessibility.flatten([tree], window: window, limit: 100, depth: 64) { (fake: Fake) in
      fake.raw.role == "AXGroup" ? nil : (fake.raw, fake.children)
    }
    #expect(unreadable.map(\.role) == ["AXWindow", "AXTextField", "AXCheckBox"])
  }
}
