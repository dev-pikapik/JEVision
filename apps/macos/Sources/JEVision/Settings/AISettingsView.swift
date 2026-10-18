import JEVisionCore
import JEVisionHands
import SwiftUI

/// Settings → AI: the model that checks screens by meaning.
struct AISettingsView: View {
  var body: some View {
    Form {
      Section {
        PageHeader(
          symbol: "sparkles", color: .purple, title: Text("AI"),
          message: Text("An AI model can check a screen from a sentence like “The cart shows 2 items”, where comparing pictures is too strict."))
      }
      AISections()
      FallbackSections()
      AIUsageSections()
      ProjectPrivacySection()
      AssistantsSection()
    }
    .formStyle(.grouped)
  }
}

/// The provider, its model and key, a connection test and what is sent where. Shared by
/// Settings and the first-launch window.
struct AISections: View {
  @AppStorage(AISettings.providerKey) private var providerID = ""
  @AppStorage(AISettings.localOnlyKey) private var localOnly = false
  private let providers = AISettings.providers

  var body: some View {
    Section {
      Picker("Provider", selection: $providerID) {
        Text("None").tag("")
        Divider()
        ForEach(providers, id: \.id) { Text(verbatim: $0.name).tag($0.id) }
      }
      Toggle(isOn: $localOnly) {
        Text("Only models on this Mac")
        Text("Screenshots never go to a cloud service. Checks that would need one stop with a message.")
      }
    } footer: {
      if providerID.isEmpty {
        Text("Without a provider, checks compare pictures only. JEVision sends nothing anywhere.")
          .font(.callout)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
    }
    if let provider = providers.first(where: { $0.id == providerID }) {
      ProviderSection(provider: provider, localOnly: localOnly)
        .id(provider.id)
    }
  }
}

/// The provider asked when the main one can't answer, with its own model and key.
private struct FallbackSections: View {
  @AppStorage(AISettings.providerKey) private var providerID = ""
  @AppStorage(AISettings.fallbackKey) private var fallbackID = ""
  @AppStorage(AISettings.localOnlyKey) private var localOnly = false
  private let providers = AISettings.providers

  var body: some View {
    if !providerID.isEmpty {
      Section {
        Picker("Fallback Provider", selection: $fallbackID) {
          Text("None").tag("")
          Divider()
          ForEach(providers.filter { $0.id != providerID }, id: \.id) { Text(verbatim: $0.name).tag($0.id) }
          if fallbackID == providerID { Text(verbatim: AISettings.provider(fallbackID)?.name ?? "").tag(fallbackID) }
        }
      } footer: {
        Text("Asked when the main provider can’t be reached, is overloaded or has reached its limit. Problems with the setup, like a wrong key, still stop the check.")
          .font(.callout)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
      if fallbackID != providerID, let fallback = providers.first(where: { $0.id == fallbackID }) {
        ProviderSection(provider: fallback, localOnly: localOnly)
          .id("fallback." + fallback.id)
      }
    }
  }
}

private struct ProviderSection: View {
  let provider: VisionProvider
  let localOnly: Bool
  @AppStorage private var model: String
  @AppStorage private var address: String
  @State private var models: [String] = []
  @State private var listing = Listing.idle
  @State private var test = Test.idle
  @State private var settingKey = false
  @State private var keyVersion = 0
  @State private var keyError: String?

  private enum Listing: Equatable {
    case idle, loading, loaded, failed(String)
  }

  private enum Test: Equatable {
    case idle, running, works(String), confused(String), failed(String)
  }

  init(provider: VisionProvider, localOnly: Bool) {
    self.provider = provider
    self.localOnly = localOnly
    _model = AppStorage(wrappedValue: provider.defaultModel, AISettings.modelKey(provider.id))
    _address = AppStorage(wrappedValue: "", AISettings.addressKey(provider.id))
  }

  /// Read when needed, not on every redraw: the key comes from the Keychain.
  private var settings: VisionSettings {
    VisionSettings(provider: provider.id, model: model, address: address, key: AISettings.key(for: provider))
  }

  private var isLocal: Bool {
    visionIsLocal(settings: VisionSettings(provider: provider.id, model: "", address: address, key: nil))
  }

  /// Cloud services have one address; local servers and compatible ones need theirs.
  private var hasAddress: Bool { !provider.needsKey }

  var body: some View {
    Section {
      if hasAddress {
        TextField(
          "Address", text: $address,
          prompt: Text(verbatim: provider.defaultAddress.isEmpty ? "http://localhost:8080/v1" : provider.defaultAddress))
      }
      modelRow
      if !provider.keyVariable.isEmpty { keyRow }
      testRow
    } header: {
      Text(verbatim: provider.name)
    } footer: {
      VStack(alignment: .leading, spacing: 6) {
        destination
        if localOnly, !isLocal {
          Label(
            "Only models on this Mac are allowed, so checks by meaning with \(provider.name) stop with a message.",
            systemImage: "exclamationmark.triangle")
            .foregroundStyle(.orange)
        }
      }
      .font(.callout)
      .foregroundStyle(.secondary)
      .fixedSize(horizontal: false, vertical: true)
    }
    .task(id: address) {
      guard provider.listsModels else { return }
      // Typing an address: wait until it stops changing.
      try? await Task.sleep(for: .milliseconds(400))
      guard !Task.isCancelled else { return }
      await loadModels()
    }
    .sheet(isPresented: $settingKey) {
      SetSecretSheet(
        name: provider.keyVariable, title: Text("API Key for \(provider.name)"),
        message: Text("The key is kept in the Keychain on this Mac and sent only to \(provider.name). JEVision never shows it."),
        service: AISettings.keyService
      ) { keyVersion += 1 }
    }
    .alert(
      "Couldn’t remove the key", isPresented: Binding(get: { keyError != nil }, set: { if !$0 { keyError = nil } }),
      presenting: keyError
    ) { _ in
      Button("OK") {}
    } message: { Text($0) }
  }

  private var destination: Text {
    if isLocal {
      Text("The model runs on this Mac: screenshots and expected texts stay here.")
    } else {
      Text(
        "For each check by meaning, JEVision sends \(provider.needsKey ? provider.name : (address.isEmpty ? provider.name : address)) the compared part of the screen, the reference image if there is one, and the expected text. Password fields and the areas the test or project hides are painted over first. Test Connection sends a small built-in picture."
      )
    }
  }

  @ViewBuilder private var modelRow: some View {
    if provider.listsModels, !models.isEmpty {
      Picker("Model", selection: $model) {
        if !models.contains(model) { Text(verbatim: model).tag(model) }
        ForEach(models, id: \.self) { Text(verbatim: $0).tag($0) }
      }
    } else {
      TextField("Model", text: $model, prompt: Text(verbatim: provider.defaultModel))
    }
    if provider.listsModels {
      HStack {
        switch listing {
        case .idle, .loaded:
          if listing == .loaded, models.isEmpty {
            Text("This server has no models that understand pictures. Download one, then refresh.")
              .foregroundStyle(.secondary)
          }
        case .loading:
          ProgressView().controlSize(.small)
          Text("Looking for models…").foregroundStyle(.secondary)
        case .failed(let message):
          Text(message).foregroundStyle(.secondary)
        }
        Spacer()
        Button("Refresh", systemImage: "arrow.clockwise") { Task { await loadModels() } }
          .disabled(listing == .loading)
      }
      .font(.callout)
      .fixedSize(horizontal: false, vertical: true)
    }
  }

  @ViewBuilder private var keyRow: some View {
    let _ = keyVersion
    let fromEnvironment = SecretStore.isInEnvironment(provider.keyVariable)
    let inKeychain = SecretStore.isInKeychain(provider.keyVariable, in: AISettings.keyService)
    let isSet = fromEnvironment || inKeychain
    LabeledContent {
      HStack {
        Label {
          Text(isSet ? "Set" : "Not set")
        } icon: {
          Image(systemName: isSet ? "checkmark.circle.fill" : "circle.dashed")
            .foregroundStyle(isSet ? .green : .secondary)
        }
        if inKeychain, !fromEnvironment {
          Button("Remove", role: .destructive) {
            do {
              try SecretStore.remove(provider.keyVariable, in: AISettings.keyService)
              keyVersion += 1
            } catch {
              keyError = error.localizedDescription
            }
          }
        }
        Button("Set…") { settingKey = true }
          .disabled(fromEnvironment)
      }
    } label: {
      Text("API Key")
      if fromEnvironment {
        Text("From the environment variable \(provider.keyVariable)")
      } else if !provider.needsKey {
        Text("Only if the server asks for one")
      }
    }
  }

  @ViewBuilder private var testRow: some View {
    LabeledContent {
      Button("Test Connection") { Task { await runTest() } }
        .disabled(test == .running)
    } label: {
      switch test {
      case .idle:
        Text("Asks the model about a small picture")
      case .running:
        HStack {
          ProgressView().controlSize(.small)
          Text("Waiting for the answer…")
        }
      case .works(let reason):
        Label {
          Text("It works. The model answered: \(reason)")
        } icon: {
          Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
        }
      case .confused(let reason):
        Label {
          Text("The model answered, but described the picture wrong: \(reason) Try a model that understands pictures better.")
        } icon: {
          Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
        }
      case .failed(let message):
        Label {
          Text(message)
        } icon: {
          Image(systemName: "xmark.octagon.fill").foregroundStyle(.red)
        }
      }
    }
  }

  private func loadModels() async {
    listing = .loading
    let settings = settings
    let result = await Task.detached { Result { try visionModels(settings: settings) } }.value
    switch result {
    case .success(let found):
      models = found
      listing = .loaded
      if model.isEmpty, let first = found.first { model = first }
    case .failure(let error):
      models = []
      listing = .failed(VisionText.error(error))
    }
  }

  private func runTest() async {
    test = .running
    let settings = settings
    let result = await Task.detached { Result { try visionTest(settings: settings) } }.value
    switch result {
    case .success(let verdict):
      test = verdict.passed ? .works(verdict.reason) : .confused(verdict.reason)
    case .failure(let error):
      test = .failed(VisionText.error(error))
    }
    let announcement =
      switch test {
      case .works: String(localized: "Connection works")
      case .confused: String(localized: "The model answered, but described the picture wrong")
      default: String(localized: "Connection failed")
      }
    AccessibilityNotification.Announcement(announcement).post()
  }
}
