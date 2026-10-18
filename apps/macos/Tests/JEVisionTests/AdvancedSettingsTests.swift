import Foundation
import Testing

@testable import JEVision

struct AdvancedSettingsTests {
  @Test func findsTheProjectInsideItsRepository() throws {
    let repo = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: repo) }
    let project = repo.appending(path: "ui tests/mac")
    try FileManager.default.createDirectory(at: project, withIntermediateDirectories: true)
    #expect(CIWorkflow.testsPath(of: project) == ".")
    try FileManager.default.createDirectory(at: repo.appending(path: ".git"), withIntermediateDirectories: true)
    #expect(CIWorkflow.testsPath(of: project) == "ui tests/mac")
    #expect(CIWorkflow.testsPath(of: repo) == ".")
  }

  @Test func workflowPassesSecretsAndQuotesThePath() {
    let plain = CIWorkflow.text(testsPath: "tests/smoke", secrets: [])
    #expect(plain.contains("          jevision run tests/smoke --junit junit.xml --html reports\n"))
    #expect(!plain.contains("env:"))
    let spaced = CIWorkflow.text(testsPath: "ui tests/it's", secrets: ["API_KEY", "PASSWORD"])
    #expect(spaced.contains(#"jevision run 'ui tests/it'\''s' --junit"#))
    #expect(spaced.contains("        env:\n          API_KEY: ${{ secrets.API_KEY }}\n          PASSWORD: ${{ secrets.PASSWORD }}\n"))
    #expect(spaced.contains("runs-on: [self-hosted, macOS]"))
  }

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
