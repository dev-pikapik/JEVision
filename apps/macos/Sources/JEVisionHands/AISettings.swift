import Foundation
import JEVisionCore

/// The AI model for checks by meaning, as chosen in Settings → AI. The app and the `jevision`
/// tool read the same choice. API keys come from the environment or the Keychain, kept apart
/// from the secrets tests type, so a test can't read them.
public struct AISettings {
  /// Where the app keeps its settings; the `jevision` tool reads them from there.
  public static let appDomain = "io.github.dev-pikapik.jevision"
  public static let keyService = "JEVision AI"
  public static let providerKey = "ai.provider"
  public static let localOnlyKey = "ai.localOnly"
  public static let assistantsRunKey = "assistants.allowRuns"
  public static let assistantsCaptureKey = "assistants.allowScreenshots"
  public static let assistantsAskFirstKey = "assistants.askFirst"
  public static let fallbackKey = "ai.fallback"
  public static let pricesKey = "ai.prices"

  public static func modelKey(_ provider: String) -> String { "ai.model.\(provider)" }
  public static func addressKey(_ provider: String) -> String { "ai.address.\(provider)" }
  public static func dailyLimitKey(_ provider: String) -> String { "ai.limit.daily.\(provider)" }
  public static func monthlyLimitKey(_ provider: String) -> String { "ai.limit.monthly.\(provider)" }
  public static func picturesKey(_ provider: String) -> String { "ai.pictures.\(provider)" }

  /// The journal of every AI call, shared by the app and the `jevision` tool.
  public static var journal: URL {
    Profile.current.supportFolder.appending(path: "ai-usage.jsonl", directoryHint: .notDirectory)
  }

  /// Where the app and every `jevision mcp` meet: who connected, and what the person is asked.
  public static var assistantsFolder: URL {
    Profile.current.supportFolder.appending(path: "Assistants", directoryHint: .isDirectory)
  }

  /// The app's settings, from the app or from the `jevision` tool.
  public static var app: AISettings { AISettings(Profile.current.appDefaults) }

  public static var providers: [VisionProvider] { visionProviders() }

  public static func provider(_ id: String) -> VisionProvider? {
    providers.first { $0.id == id }
  }

  let defaults: UserDefaults

  public init(_ defaults: UserDefaults) {
    self.defaults = defaults
  }

  /// The chosen provider's id; empty when checks by meaning are off.
  public var providerID: String { defaults.string(forKey: Self.providerKey) ?? "" }

  /// Checks by meaning may only use a model on this Mac.
  public var localOnly: Bool { defaults.bool(forKey: Self.localOnlyKey) }

  /// AI assistants (`jevision mcp`) may run tests, which click and type on this Mac.
  public var assistantsMayRun: Bool { defaults.bool(forKey: Self.assistantsRunKey) }

  /// AI assistants may take screenshots of app windows.
  public var assistantsMayCapture: Bool { defaults.bool(forKey: Self.assistantsCaptureKey) }

  /// Whether the person is asked before each run an assistant starts; on until they turn it off.
  public var assistantsAskFirst: Bool { defaults.object(forKey: Self.assistantsAskFirstKey) as? Bool ?? true }

  /// Picks up what the app changed since this process last read its settings.
  public func refresh() {
    CFPreferencesAppSynchronize(Profile.current.appDefaultsDomain as CFString)
  }

  /// The chosen model, or the provider's suggestion.
  public func model(for provider: VisionProvider) -> String {
    let saved = defaults.string(forKey: Self.modelKey(provider.id)) ?? ""
    return saved.isEmpty ? provider.defaultModel : saved
  }

  /// The server address; empty for the provider's own.
  public func address(for provider: VisionProvider) -> String {
    defaults.string(forKey: Self.addressKey(provider.id)) ?? ""
  }

  /// The model understands pictures, unless the user said it reads only text.
  public func pictures(for provider: VisionProvider) -> Bool {
    defaults.object(forKey: Self.picturesKey(provider.id)) as? Bool ?? true
  }

  /// What a run passes to the core: the chosen provider and model unless others are given.
  /// Nil when no provider is chosen.
  public func settings(provider id: String? = nil, model: String? = nil) -> VisionSettings? {
    guard let provider = Self.provider(id ?? providerID) else { return nil }
    return VisionSettings(
      provider: provider.id, model: model ?? self.model(for: provider), address: address(for: provider),
      key: Self.key(for: provider), pictures: pictures(for: provider))
  }

  /// The AI that writes tests and explains failures: the one the project names for it, else the
  /// chosen provider. Nil when neither names a provider.
  public func textSettings(project: ProjectSettings) -> VisionSettings? {
    let choice = project.aiText
    return settings(
      provider: choice.flatMap { $0.provider.isEmpty ? nil : $0.provider },
      model: choice.flatMap { $0.model.isEmpty ? nil : $0.model })
  }

  /// What a session asking `settings` also gets: the fallback provider, and the journal and the
  /// limits its calls count against.
  public func safeguards(for settings: VisionSettings) -> AiSafeguards {
    AiSafeguards(fallback: fallback(besides: settings.provider), usage: usage)
  }

  /// The provider that finds the elements tests describe in words, whichever one is chosen.
  public static let finder = "jev"

  /// The settings of the providers that checks by meaning name and that blocks need besides the
  /// chosen one, read only when one is needed, so a run reads no API key it doesn't need.
  public var namedProviders: ProviderSettings { NamedProviders(settings: self) }

  /// The provider asked when the main one can't answer; empty for none.
  public var fallbackID: String { defaults.string(forKey: Self.fallbackKey) ?? "" }

  /// What a run passes to the core as the fallback: nil when there is none or it is `main`.
  public func fallback(besides main: String? = nil) -> VisionSettings? {
    let id = fallbackID
    guard !id.isEmpty, id != (main ?? providerID) else { return nil }
    return settings(provider: id)
  }

  /// The price table: the user's, or the public prices JEVision starts from.
  public var prices: [Price] {
    guard let rows = defaults.array(forKey: Self.pricesKey) as? [[String: Any]] else { return aiDefaultPrices() }
    return rows.compactMap { row in
      guard let provider = row["provider"] as? String, let model = row["model"] as? String,
        let input = row["input"] as? Double, let output = row["output"] as? Double
      else { return nil }
      return Price(provider: provider, model: model, input: input, output: output)
    }
  }

  /// Saves the user's price table.
  public static func save(prices: [Price], in defaults: UserDefaults) {
    let rows: [[String: Any]] = prices.map {
      ["provider": $0.provider, "model": $0.model, "input": $0.input, "output": $0.output]
    }
    defaults.set(rows, forKey: pricesKey)
  }

  /// Spending limits in US dollars, for the providers that have one.
  public var limits: [Limit] {
    Self.providers.compactMap { provider in
      let daily = defaults.object(forKey: Self.dailyLimitKey(provider.id)) as? Double
      let monthly = defaults.object(forKey: Self.monthlyLimitKey(provider.id)) as? Double
      guard daily != nil || monthly != nil else { return nil }
      return Limit(provider: provider.id, daily: daily, monthly: monthly)
    }
  }

  /// Where a run logs its AI calls, at what prices and within which limits.
  public var usage: AiUsageSettings {
    AiUsageSettings(journal: Self.journal.path(percentEncoded: false), prices: prices, limits: limits)
  }

  /// The provider has an API key in the environment or the Keychain, without reading it.
  public static func hasKey(_ provider: VisionProvider) -> Bool {
    !provider.keyVariable.isEmpty
      && (SecretStore.isInEnvironment(provider.keyVariable)
        || SecretStore.isInKeychain(provider.keyVariable, in: keyService))
  }

  /// The API key, from the provider's environment variable or the Keychain.
  public static func key(for provider: VisionProvider) -> String? {
    provider.keyVariable.isEmpty ? nil : SecretStore.value(provider.keyVariable, in: keyService)
  }
}

/// UserDefaults and the Keychain may be read from any thread, so the run's thread asks here.
private final class NamedProviders: ProviderSettings, @unchecked Sendable {
  let settings: AISettings

  init(settings: AISettings) {
    self.settings = settings
  }

  func settings(provider: String) -> VisionSettings? {
    settings.settings(provider: provider)
  }
}
