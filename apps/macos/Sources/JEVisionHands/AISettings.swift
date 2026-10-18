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

  public static func modelKey(_ provider: String) -> String { "ai.model.\(provider)" }
  public static func addressKey(_ provider: String) -> String { "ai.address.\(provider)" }

  /// The app's settings, from the app or from the `jevision` tool.
  public static var app: AISettings {
    let own = Bundle.main.bundleIdentifier == appDomain
    return AISettings(own ? .standard : UserDefaults(suiteName: appDomain) ?? .standard)
  }

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

  /// The chosen model, or the provider's suggestion.
  public func model(for provider: VisionProvider) -> String {
    let saved = defaults.string(forKey: Self.modelKey(provider.id)) ?? ""
    return saved.isEmpty ? provider.defaultModel : saved
  }

  /// The server address; empty for the provider's own.
  public func address(for provider: VisionProvider) -> String {
    defaults.string(forKey: Self.addressKey(provider.id)) ?? ""
  }

  /// What a run passes to the core: the chosen provider and model unless others are given.
  /// Nil when no provider is chosen.
  public func settings(provider id: String? = nil, model: String? = nil) -> VisionSettings? {
    guard let provider = Self.provider(id ?? providerID) else { return nil }
    return VisionSettings(
      provider: provider.id, model: model ?? self.model(for: provider), address: address(for: provider),
      key: Self.key(for: provider))
  }

  /// The API key, from the provider's environment variable or the Keychain.
  public static func key(for provider: VisionProvider) -> String? {
    provider.keyVariable.isEmpty ? nil : SecretStore.value(provider.keyVariable, in: keyService)
  }
}
