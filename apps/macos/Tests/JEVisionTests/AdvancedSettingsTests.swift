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
    defaults.set("Smoke", forKey: ProjectStore.Key.name)
    SettingsReset.reset(defaults, domain: suite)
    #expect(defaults.object(forKey: "appearance") == nil)
    #expect(defaults.bool(forKey: OnboardingView.doneKey))
    #expect(defaults.string(forKey: ProjectStore.Key.name) == "Smoke")
  }
}
