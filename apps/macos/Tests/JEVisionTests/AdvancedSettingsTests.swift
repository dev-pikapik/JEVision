import Foundation
import Testing

@testable import JEVision

struct AdvancedSettingsTests {
  @MainActor @Test func resetKeepsTheProject() throws {
    let suite = "jevision.tests.\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    defer { defaults.removePersistentDomain(forName: suite) }
    defaults.set("dark", forKey: "appearance")
    defaults.set(true, forKey: OnboardingView.doneKey)
    defaults.set("[]", forKey: ProjectStore.Key.recent)
    defaults.set("/Users/me/Smoke", forKey: ProjectStore.Key.current)
    try SettingsReset.reset(defaults, domain: suite)
    #expect(defaults.object(forKey: "appearance") == nil)
    #expect(defaults.bool(forKey: OnboardingView.doneKey))
    #expect(defaults.string(forKey: ProjectStore.Key.recent) == "[]")
    #expect(defaults.string(forKey: ProjectStore.Key.current) == "/Users/me/Smoke")
  }
}
