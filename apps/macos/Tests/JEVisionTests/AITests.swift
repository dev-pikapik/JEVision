import Foundation
import JEVisionCore
import JEVisionHands
import Testing

@testable import JEVision

struct AISettingsTests {
  @Test func runsUseTheChosenProviderModelAndAddress() throws {
    let suite = "jevision-tests-\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    defer { defaults.removePersistentDomain(forName: suite) }
    let ai = AISettings(defaults)
    #expect(ai.settings() == nil)
    #expect(!ai.localOnly)

    defaults.set("ollama", forKey: AISettings.providerKey)
    defaults.set("qwen2.5vl:7b", forKey: AISettings.modelKey("ollama"))
    defaults.set("http://127.0.0.1:11500", forKey: AISettings.addressKey("ollama"))
    defaults.set(true, forKey: AISettings.localOnlyKey)
    #expect(
      ai.settings() == VisionSettings(provider: "ollama", model: "qwen2.5vl:7b", address: "http://127.0.0.1:11500", key: nil, pictures: true))
    #expect(ai.localOnly)
    defaults.set(false, forKey: AISettings.picturesKey("ollama"))
    #expect(ai.settings()?.pictures == false)
    #expect(ai.settings(provider: "lmstudio")?.pictures == true)

    // The command line picks another provider or model for one run; unset models fall back to
    // the provider's suggestion.
    let gemini = try #require(ai.settings(provider: "gemini"))
    #expect(gemini.model == "gemini-2.5-flash")
    #expect(gemini.address.isEmpty)
    #expect(ai.settings(model: "llava")?.model == "llava")
    #expect(ai.settings(provider: "skynet") == nil)
  }

  @Test func everyProviderCanBeChosen() {
    let providers = AISettings.providers
    #expect(providers.map(\.name) == ["Anthropic", "OpenAI", "Google Gemini", "JEV (TypeSafe AI)", "Ollama", "LM Studio", "OpenAI-compatible server"])
    // Cloud services need a key; local servers list their models.
    for provider in providers where provider.needsKey {
      #expect(!provider.keyVariable.isEmpty && provider.defaultAddress.hasPrefix("https://"))
    }
    #expect(providers.filter(\.listsModels).map(\.id) == ["ollama", "lmstudio", "openai-compatible"])
    #expect(providers.filter(\.mayLackPictures).map(\.id) == ["jev", "ollama", "lmstudio", "openai-compatible"])
  }
}

@MainActor
struct VisionTextTests {
  private let codes = [
    "run.vision.noPictures", "run.vision.noModel", "run.vision.noKey", "run.vision.badAddress", "run.vision.unreachable", "run.vision.timeout",
    "run.vision.badKey", "run.vision.unknownModel", "run.vision.rateLimited", "run.vision.serviceError",
    "run.vision.refused", "run.vision.badAnswer", "run.vision.cannotLocate", "run.vision.modelCannotLocate",
  ]

  @Test func everyProblemHasItsOwnWords() {
    let params = [
      "provider": "Ollama", "model": "llava", "address": "http://localhost:11434", "seconds": "120", "status": "500",
      "message": "Out of memory.", "answer": "maybe",
    ]
    var texts = Set<String>()
    for code in codes {
      let text = VisionText.failure(Failure(code: code, params: params))
      #expect(!text.contains("run.vision"), "\(code)")
      texts.insert(text)
      var inBlock = params
      inBlock["block"] = "cart"
      #expect(RunText.failure(Failure(code: code, params: inBlock)) == "Block “cart”: \(text)")
    }
    #expect(texts.count == codes.count)
    #expect(VisionText.failure(Failure(code: "run.vision.timeout", params: params)) == "Ollama didn’t answer within 120 s.")
  }

  @Test func setupProblemsLeadToSettings() {
    let blocked = Failure(code: "run.vision.cloudBlocked", params: ["block": "b", "provider": "OpenAI"])
    #expect(RunText.needsAISetup(blocked))
    #expect(RunText.failure(blocked).contains("OpenAI"))
    #expect(RunText.needsAISetup(Failure(code: "run.compare.meaningUnavailable", params: ["block": "b"])))
    #expect(!RunText.needsAISetup(Failure(code: "run.vision.timeout", params: [:])))
    let mismatch = Failure(code: "run.check.meaningMismatch", params: ["block": "b", "provider": "Anthropic", "reason": "The cart is empty."])
    #expect(RunText.failure(mismatch) == "Block “b”: the screen doesn’t show what’s expected. Anthropic: The cart is empty.")
  }

  @Test func connectionErrorsUseTheSameWords() {
    let error = VisionCallError.Failed(
      failure: Failure(code: "run.vision.noKey", params: ["provider": "OpenAI", "model": "gpt-5-mini"]),
      message: "The API key for OpenAI is not set.")
    #expect(VisionText.error(error) == "The API key for OpenAI isn’t set. Set it in Settings → AI.")
  }

  @Test func checksByMeaningReadAsTheirExpectation() {
    let reference = Reference(
      image: "", compare: .meaning(expect: "The cart shows 2 items"), area: nil, ignore: [], hide: [])
    #expect(BlockSummary.text(.check(reference: reference, next: nil), appName: { $0 }) == "Screen shows “The cart shows 2 items”")
  }
}
