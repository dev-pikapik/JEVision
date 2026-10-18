import Testing

@testable import JEVision

@MainActor
struct SettingsTests {
  @Test func backAndForwardFollowTheVisitedPanes() {
    let model = SettingsModel(selection: .general)
    model.selection = .ai
    model.selection = .git
    model.goBack()
    #expect(model.selection == .ai)
    model.goBack()
    #expect(model.selection == .general)
    #expect(model.back.isEmpty)
    model.goForward()
    #expect(model.selection == .ai)
    model.selection = .advanced
    #expect(model.forward.isEmpty)
    #expect(model.back == [.general, .ai])
  }

  @Test func searchJumpsToThePaneOfTheSetting() throws {
    #expect(Set(SettingsItem.all.map(\.id)).count == SettingsItem.all.count)
    let model = SettingsModel(selection: .general)
    let relay = try #require(SettingsItem.all.first { $0.id == "relay" })
    model.open(relay)
    #expect(model.selection == .remote)
    #expect(model.highlight == "relay")
  }
}
