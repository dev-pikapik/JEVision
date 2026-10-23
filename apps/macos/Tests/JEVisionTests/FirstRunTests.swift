import Foundation
import JEVisionCore
import JEVisionHands
import Testing

@testable import JEVision

@MainActor
struct FirstRunTests {
  /// The report a toast, a chip or a block opens is the one the history row of that run picks.
  @Test func openedRunMatchesItsHistoryRow() {
    let run = SavedRun(
      dir: "/p/.jevision/runs/2026-10-18T10:00:00Z",
      report: RunReport(
        test: "a", file: "a.flow.json", app: "com.example.App", startedAt: "2026-10-18T10:00:00Z", durationMs: 1,
        verdict: .passed, flaky: false, baselinesCreated: [], attempts: []))
    let project = ProjectStore()
    project.showRun(run.url)
    #expect(project.sidebar == .runs)
    #expect(project.selectedRun == run.id)
  }

  @Test func savedProviderPicksItsCard() {
    #expect(AIChoice(provider: "") == nil)
    #expect(AIChoice(provider: AISettings.defaultFinder) == .jev)
    #expect(AIChoice(provider: "jev-cloudflare") == .jev)
    #expect(AIChoice.jevRoutes.allSatisfy { AISettings.provider($0)?.findsElements == true })
    #expect(AIChoice(provider: "gemini") == .service)
    #expect(AIChoice(provider: "openai-compatible") == .service)
    #expect(AIChoice(provider: "lmstudio") == .local)
  }

  @Test func choosingACardKeepsItsProviderOrPicksTheFirst() {
    #expect(AIChoice.service.provider(keeping: "openai", running: []) == "openai")
    #expect(AIChoice.service.provider(keeping: "", running: []) == "anthropic")
    #expect(AIChoice.service.provider(keeping: "ollama", running: []) == "anthropic")
    #expect(AIChoice.jev.provider(keeping: "gemini", running: []) == AISettings.defaultFinder)
    #expect(AIChoice.jev.provider(keeping: "jev-cloudflare", running: []) == "jev-cloudflare")
    #expect(AIChoice.service.provider(keeping: "jev-cloudflare", running: []) == "anthropic")
    #expect(AIChoice.local.provider(keeping: "jev", running: []) == "ollama")
    #expect(AIChoice.local.provider(keeping: "", running: ["lmstudio"]) == "lmstudio")
    #expect(AIChoice.local.provider(keeping: "lmstudio", running: ["ollama"]) == "lmstudio")
  }

  @Test func onlyTheLocalCardKeepsScreenshotsOnThisMac() {
    #expect(AIChoice.allCases.filter(\.isLocalOnly) == [.local])
  }
}
