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
      ai.settings() == VisionSettings(provider: "ollama", model: "qwen2.5vl:7b", address: "http://127.0.0.1:11500", key: nil))
    #expect(ai.localOnly)

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
  }
}

@MainActor
struct VisionTextTests {
  private let codes = [
    "run.vision.noModel", "run.vision.noKey", "run.vision.badAddress", "run.vision.unreachable", "run.vision.timeout",
    "run.vision.badKey", "run.vision.unknownModel", "run.vision.rateLimited", "run.vision.serviceError",
    "run.vision.refused", "run.vision.badAnswer",
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
