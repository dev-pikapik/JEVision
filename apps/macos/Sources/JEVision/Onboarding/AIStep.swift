import JEVisionCore
import JEVisionHands
import SwiftUI

/// The three ways to get checks by meaning, as cards on the first-launch AI step.
enum AIChoice: CaseIterable, Identifiable {
  /// JEV, which also finds elements described in words.
  case jev
  /// Anthropic, OpenAI or Google, with the person's own key.
  case service
  /// A model on this Mac, so screenshots never leave it.
  case local

  var id: Self { self }

  /// JEV straight from TypeSafe AI, or through the person's Cloudflare account.
  static let jevRoutes = [AISettings.defaultFinder, "jev-cloudflare"]
  static let services = ["anthropic", "openai", "gemini"]
  static let localServers = ["ollama", "lmstudio"]

  /// The card the saved provider belongs to; `nil` when none is chosen.
  init?(provider: String) {
    switch provider {
    case "": return nil
    case _ where Self.jevRoutes.contains(provider): self = .jev
    case _ where Self.localServers.contains(provider): self = .local
    default: self = .service
    }
  }

  /// The provider choosing the card saves: the one already saved if it belongs to the card,
  /// else the first of the card, a running local server before one that isn't.
  func provider(keeping saved: String, running: Set<String>) -> String {
    if AIChoice(provider: saved) == self { return saved }
    switch self {
    case .jev: return Self.jevRoutes[0]
    case .service: return Self.services[0]
    case .local: return Self.localServers.first { running.contains($0) } ?? Self.localServers[0]
    }
  }

  /// Screenshots may only go to a model on this Mac.
  var isLocalOnly: Bool { self == .local }

  var symbol: String {
    switch self {
    case .jev: "sparkles"
    case .service: "cloud.fill"
    case .local: "laptopcomputer"
    }
  }

  var color: Color {
    switch self {
    case .jev: .purple
    case .service: .blue
    case .local: .green
    }
  }

  var title: Text {
    switch self {
    case .jev: Text(verbatim: "JEV")
    case .service: Text("Another AI service")
    case .local: Text("Only on this Mac")
    }
  }

  var summary: Text {
    switch self {
    case .jev: Text("Understands screenshots and finds what you describe in words, like “the blue Save button”.")
    case .service: Text("Anthropic, OpenAI or Google, with your own key.")
    case .local: Text("Ollama or LM Studio. Screenshots never leave this Mac.")
    }
  }
}

/// Which local model servers answer on their usual address.
enum LocalServers {
  static func running() async -> Set<String> {
    await Task.detached {
      Set(
        AIChoice.localServers.filter { id in
          (try? visionModels(settings: VisionSettings(provider: id, model: "", address: "", key: nil, pictures: true))) != nil
        })
    }.value
  }
}

/// Third onboarding step: pick where checks by meaning come from, or skip it.
struct AIStep: View {
  @AppStorage(AISettings.providerKey) private var providerID = ""
  @AppStorage(AISettings.localOnlyKey) private var localOnly = false
  /// Local servers that answered; `nil` until the first look.
  @State private var running: Set<String>?
  @State private var isLooking = false
  private let providers = AISettings.providers

  private var current: AIChoice? { AIChoice(provider: providerID) }

  var body: some View {
    VStack(spacing: 0) {
      PageHeader(
        symbol: "sparkles", color: .purple, title: Text("Checks by meaning"),
        message: Text("Optional. An AI model can check a screen from a sentence like “The cart shows 2 items”. Choose one now, or later in Settings → AI."),
        tileSize: 64)
      Form {
        Section {
          ForEach(AIChoice.allCases) { card($0) }
        } footer: {
          Text("Keys are kept in the Keychain on this Mac. They never get into tests, logs, reports or git.")
            .font(.callout)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        }
        if let current {
          details(of: current)
        }
      }
      .formStyle(.grouped)
      .scrollContentBackground(.hidden)
    }
    .padding(.horizontal, 32)
    .padding(.top, 28)
    .task { await lookForServers() }
  }

  private func card(_ choice: AIChoice) -> some View {
    let isSelected = current == choice
    return VStack(alignment: .leading, spacing: 6) {
      Button { select(choice) } label: {
        HStack(alignment: .top, spacing: 12) {
          IconTile(symbol: choice.symbol, color: choice.color, size: 36)
          VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 8) {
              choice.title.font(.headline)
              if choice == .jev {
                Text("Recommended")
                  .font(.caption.bold())
                  .padding(.horizontal, 7)
                  .padding(.vertical, 1)
                  .background(.tint.opacity(0.18), in: .capsule)
              }
            }
            choice.summary
              .font(.subheadline)
              .foregroundStyle(.secondary)
              .fixedSize(horizontal: false, vertical: true)
          }
          Spacer(minLength: 8)
          Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
            .imageScale(.large)
            .foregroundStyle(isSelected ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
            .accessibilityHidden(true)
        }
        .contentShape(.rect)
      }
      .buttonStyle(.plain)
      .accessibilityAddTraits(isSelected ? .isSelected : [])
      if choice == .local { serverStatus.padding(.leading, 48) }
    }
    .listRowBackground(isSelected ? Color.accentColor.opacity(0.12) : nil)
  }

  /// Whether Ollama and LM Studio answer, with a symbol as well as words.
  private var serverStatus: some View {
    HStack(spacing: 10) {
      ForEach(AIChoice.localServers, id: \.self) { id in
        let name = providers.first { $0.id == id }?.name ?? id
        if let running {
          if running.contains(id) {
            Label("\(name) is running", systemImage: "checkmark.circle.fill")
          } else {
            Label("\(name) isn’t running", systemImage: "circle.dashed")
          }
        }
      }
      if running == nil {
        ProgressView().controlSize(.small)
        Text("Looking for models on this Mac…")
      } else {
        Button("Check Again") { Task { await lookForServers() } }
          .buttonStyle(.link)
          .disabled(isLooking)
      }
    }
    .font(.caption)
    .foregroundStyle(.secondary)
  }

  @ViewBuilder private func details(of choice: AIChoice) -> some View {
    switch choice {
    case .jev:
      Section {
        Picker("Provider", selection: $providerID) {
          ForEach(providerChoices(AIChoice.jevRoutes), id: \.id) { Text(verbatim: $0.name).tag($0.id) }
        }
        .pickerStyle(.segmented)
      } footer: {
        Text("Use a TypeSafe AI key, or reach JEV through Workers AI with your Cloudflare account.")
          .font(.callout)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
    case .service:
      Section {
        Picker("Service", selection: $providerID) {
          ForEach(providerChoices(AIChoice.services), id: \.id) { Text(verbatim: $0.name).tag($0.id) }
        }
      }
    case .local:
      Section {
        Picker("Program", selection: $providerID) {
          ForEach(providerChoices(AIChoice.localServers), id: \.id) { Text(verbatim: $0.name).tag($0.id) }
        }
        .pickerStyle(.segmented)
      }
    }
    if let provider = providers.first(where: { $0.id == providerID }) {
      ProviderSection(provider: provider, localOnly: localOnly)
        .id(provider.id)
    }
    Section {
      Button("Don’t Use AI") {
        providerID = ""
        localOnly = false
      }
      .buttonStyle(.link)
    }
  }

  /// The providers of a card, plus the saved one when it is another, so the picker shows it.
  private func providerChoices(_ ids: [String]) -> [VisionProvider] {
    providers.filter { ids.contains($0.id) || $0.id == providerID }
  }

  private func select(_ choice: AIChoice) {
    providerID = choice.provider(keeping: providerID, running: running ?? [])
    localOnly = choice.isLocalOnly
  }

  private func lookForServers() async {
    isLooking = true
    running = await LocalServers.running()
    isLooking = false
  }
}
