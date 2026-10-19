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

  @Test func runsKeepToTheLimitsPricesAndFallback() throws {
    let suite = "jevision-tests-\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    defer { defaults.removePersistentDomain(forName: suite) }
    let ai = AISettings(defaults)
    #expect(ai.prices == aiDefaultPrices())
    #expect(ai.limits.isEmpty)
    #expect(ai.fallback() == nil)

    defaults.set("anthropic", forKey: AISettings.providerKey)
    defaults.set("anthropic", forKey: AISettings.fallbackKey)
    // The main provider can't be its own fallback.
    #expect(ai.fallback() == nil)
    defaults.set("ollama", forKey: AISettings.fallbackKey)
    defaults.set("llava", forKey: AISettings.modelKey("ollama"))
    #expect(ai.fallback()?.provider == "ollama")
    #expect(ai.fallback()?.model == "llava")
    #expect(ai.fallback(besides: "ollama") == nil)

    defaults.set(5.0, forKey: AISettings.dailyLimitKey("anthropic"))
    defaults.set(40.0, forKey: AISettings.monthlyLimitKey("openai"))
    #expect(
      ai.limits == [
        Limit(provider: "anthropic", daily: 5, monthly: nil), Limit(provider: "openai", daily: nil, monthly: 40),
      ])

    let prices = [Price(provider: "anthropic", model: "claude-sonnet", input: 2.5, output: 12)]
    AISettings.save(prices: prices, in: defaults)
    #expect(ai.prices == prices)
    #expect(ai.usage.prices == prices)
    #expect(ai.usage.journal.hasSuffix("JEVision/ai-usage.jsonl"))
    #expect(aiDefaultPrices().contains { $0.provider == "anthropic" && $0.input > 0 })
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
    "run.vision.refused", "run.vision.badAnswer", "run.vision.limitReached", "run.vision.limitNear",
    "run.vision.journalFailed",
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
    let limit = Failure(
      code: "run.vision.limitReached",
      params: ["block": "b", "provider": "OpenAI", "period": "month", "limit": "40.00", "spent": "40.12"])
    #expect(RunText.needsAISetup(limit))
    #expect(VisionText.failure(limit).hasPrefix("OpenAI has used $40.12 of its monthly limit of $40.00."))
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
      image: "", compare: .meaning(expect: "The cart shows 2 items", model: "", provider: ""), area: nil, ignore: [],
      hide: [])
    #expect(BlockSummary.text(.check(reference: reference, next: nil), appName: { $0 }) == "Screen shows “The cart shows 2 items”")
    let named = Reference(
      image: "", compare: .meaning(expect: "The cart shows 2 items", model: "", provider: "ollama"), area: nil,
      ignore: [], hide: [])
    #expect(BlockSummary.text(.check(reference: named, next: nil), appName: { $0 }) == "Screen shows “The cart shows 2 items” · Ollama")
  }
}
