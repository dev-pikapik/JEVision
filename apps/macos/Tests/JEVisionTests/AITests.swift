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

  @Test func textsAskTheProjectsAIBeforeTheChosenOne() throws {
    let suite = "jevision-tests-\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    defer { defaults.removePersistentDomain(forName: suite) }
    let ai = AISettings(defaults)
    var project = ProjectSettings(retries: nil, quarantine: [])
    #expect(ai.textSettings(project: project) == nil)

    defaults.set("anthropic", forKey: AISettings.providerKey)
    #expect(ai.textSettings(project: project)?.provider == "anthropic")
    defaults.set("llava", forKey: AISettings.modelKey("ollama"))
    project.aiText = AiChoice(provider: "ollama", model: "")
    #expect(ai.textSettings(project: project)?.provider == "ollama")
    #expect(ai.textSettings(project: project)?.model == "llava", "without a model the project leaves it to Settings")
    project.aiText = AiChoice(provider: "ollama", model: "qwen2.5vl:7b")
    #expect(ai.textSettings(project: project)?.model == "qwen2.5vl:7b")

    // A fallback is for another provider than the one in force, and brings the spending limits.
    defaults.set("ollama", forKey: AISettings.fallbackKey)
    let settings = try #require(ai.textSettings(project: project))
    #expect(ai.safeguards(for: settings).fallback == nil)
    defaults.set("openai", forKey: AISettings.fallbackKey)
    #expect(ai.safeguards(for: settings).fallback?.provider == "openai")
    #expect(ai.safeguards(for: settings).usage == ai.usage)
  }

  @Test func theProjectKeepsOnlyTheNameOfItsAI() throws {
    let folder = FileManager.default.temporaryDirectory.appending(path: "jevision-ai-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: folder) }
    let path = folder.path(percentEncoded: false)

    let screen = AiChoice(provider: "ollama", model: "llava")
    let saved = try setProjectAi(project: path, task: .screen, choice: screen)
    #expect(saved.aiScreen == screen && saved.aiText == nil)
    let file = try String(contentsOf: folder.appending(path: projectSettingsFileName()), encoding: .utf8)
    #expect(file.contains("ollama") && file.contains("llava"))
    #expect(!file.lowercased().contains("key"), "the file holds the provider and the model, never a key")

    // A block's own choice comes first, then the project's, then Settings', which empty stands for.
    let none = AiChoice(provider: "", model: "")
    #expect(resolveAiChoice(settings: saved, task: .screen, block: none) == screen)
    #expect(resolveAiChoice(settings: saved, task: .screen, block: AiChoice(provider: "", model: "bigger")) == AiChoice(provider: "ollama", model: "bigger"))
    #expect(resolveAiChoice(settings: saved, task: .screen, block: AiChoice(provider: "openai", model: "")) == AiChoice(provider: "openai", model: ""))
    #expect(resolveAiChoice(settings: saved, task: .text, block: none) == none)

    // JEV only reads screenshots.
    #expect(throws: (any Error).self) { try setProjectAi(project: path, task: .text, choice: AiChoice(provider: "jev", model: "")) }
    #expect(try setProjectAi(project: path, task: .screen, choice: nil).aiScreen == nil)
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
    "run.vision.journalFailed", "run.vision.cannotLocate", "run.vision.modelCannotLocate",
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

@MainActor
struct DraftPreviewTests {
  private static let test = """
    {"format": 1, "name": "T", "app": "com.apple.TextEdit", "start": "a", "blocks": {
      "a": {"type": "pressKeys", "keys": "a"}}}
    """
  private static let draft = """
    {"format": 1, "name": "T", "app": "com.apple.TextEdit", "start": "x", "blocks": {
      "x": {"type": "pressKeys", "keys": "x", "next": "y"},
      "y": {"type": "pressKeys", "keys": "y"}}}
    """

  private func document() throws -> (FlowDocument, UndoManager, URL) {
    let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let url = folder.appending(path: "t.flow.json")
    try Self.test.write(to: url, atomically: true, encoding: .utf8)
    let document = FlowDocument(url: url)
    let undo = UndoManager()
    undo.groupsByEvent = false
    document.undoManager = undo
    return (document, undo, url)
  }

  private func saved(_ url: URL) throws -> [String] {
    try loadFlow(json: String(contentsOf: url, encoding: .utf8)).blocks.map(\.id).sorted()
  }

  @Test func aDraftIsShownButNotSavedUntilAccepted() throws {
    let (document, undo, url) = try document()
    document.showDraft(try loadFlow(json: Self.draft))
    #expect(document.order == ["x", "y"])
    #expect(try saved(url) == ["a"])
    // Edits to the draft stay in it.
    document.delete("y")
    #expect(document.order == ["x"])
    #expect(try saved(url) == ["a"])
    #expect(!undo.canUndo)

    undo.beginUndoGrouping()
    document.acceptDraft()
    undo.endUndoGrouping()
    #expect(document.beforeDraft == nil)
    #expect(try saved(url) == ["x"])
    undo.undo()
    #expect(document.order == ["a"])
    #expect(try saved(url) == ["a"])
  }

  @Test func cancelOrUndoPutsTheTestBack() throws {
    let (document, undo, url) = try document()
    document.showDraft(try loadFlow(json: Self.draft))
    document.discardDraft()
    #expect(document.order == ["a"])
    #expect(document.beforeDraft == nil)

    undo.beginUndoGrouping()
    document.insert("click", at: Slot(from: "a", edge: "next"))
    undo.endUndoGrouping()
    document.showDraft(try loadFlow(json: Self.draft))
    undo.undo()
    #expect(document.beforeDraft == nil, "Undo changes the test, so the draft goes")
    #expect(document.order == ["a"])
    #expect(try saved(url) == ["a"])
  }

  @Test func theAIIsOneStepAway() {
    #expect(PaletteChoice.search("describe").first == .describe)
    #expect(PaletteChoice.grouped().flatMap(\.choices).contains(.describe))
    var state = PaletteState()
    state.hasProject = true
    #expect(CommandPalette.search("describe", in: CommandPalette.entries(state)).first?.action == .describeTest)
  }
}

struct MCPServersTests {
  @Test func serversAreKeptInTheSettings() throws {
    let suite = "jevision-tests-\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    defer { defaults.removePersistentDomain(forName: suite) }
    #expect(MCPServers.load(from: defaults).isEmpty)
    let tracker = MCPServerEntry(name: "tracker", kind: .command, command: "python3 tracker.py")
    let web = MCPServerEntry(name: "web", kind: .http, url: "http://127.0.0.1:9/mcp")
    MCPServers.save([tracker, web], to: defaults)
    let loaded = MCPServers.load(from: defaults)
    #expect(loaded == [tracker, web])
    #expect(loaded[0].server == McpServer(name: "tracker", transport: .command(command: "python3 tracker.py")))
    #expect(loaded[1].location == "http://127.0.0.1:9/mcp")
  }
}

@MainActor
struct AssistantRequestsTests {
  private func request(tests: [String], environment: String? = nil, remote: String? = nil) -> AssistantRequest {
    AssistantRequest(
      id: "1", assistant: "Claude Code", remote: remote, project: "/Users/me/Shop", tests: tests,
      apps: ["com.example.mail", "com.example.notes"], environment: environment, expiresAt: 0)
  }

  @Test func theQuestionSaysWhoWantsToDoWhat() {
    let one = request(tests: ["login/sign-in.flow.json"], environment: "staging")
    #expect(AssistantRequests.title(one).contains("Claude Code"))
    let text = AssistantRequests.details(one) { $0.replacing("com.example.", with: "").capitalized }
    #expect(text.contains("login/sign-in\n"), "the test is named without its file ending")
    #expect(text.contains("Shop") && !text.contains("/Users/me"))
    #expect(text.contains("Mail") && text.contains("Notes"))
    #expect(text.contains("staging"))
    #expect(!text.contains("…"), "nothing is left out of a single test")

    let many = AssistantRequests.details(request(tests: (1...8).map { "t\($0).flow.json" })) { $0 }
    #expect(many.contains("t5\n") && !many.contains("t6"), "five tests are named, the rest counted")
    #expect(many.contains("3"))
  }

  @Test func anAssistantOnAnotherComputerSaysWhichOne() {
    let near = AssistantRequests.details(request(tests: ["a.flow.json"])) { $0 }
    #expect(!near.contains("Anna’s MacBook") && !near.contains("network"))
    let far = AssistantRequests.details(request(tests: ["a.flow.json"], remote: "Anna’s MacBook")) { $0 }
    #expect(far.contains("Anna’s MacBook") && far.contains("network"))
  }

  @Test func theAppAndTheServersMeetInOneFolder() throws {
    let folder = FileManager.default.temporaryDirectory.appending(path: "jevision-assistants-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: folder) }
    let path = folder.path(percentEncoded: false)
    #expect(connectedAssistants(folder: path).isEmpty)
    #expect(pendingAssistantRequests(folder: path).isEmpty)
    #expect(FileManager.default.fileExists(atPath: folder.appending(path: "listening").path), "polling tells the servers the app is open")
    #expect(!answerAssistantRequest(folder: path, id: "1-2-3", allow: true), "nobody asked")
    #expect(AISettings.assistantsFolder.path.hasSuffix("JEVision/Assistants"))
  }
}
