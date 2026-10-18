import Charts
import JEVisionCore
import JEVisionHands
import SwiftUI

/// Amounts of money spent on AI, in US dollars like the price table.
enum AIMoney {
  static func text(_ amount: Double) -> String {
    // Single checks cost fractions of a cent.
    amount.formatted(.currency(code: "USD").precision(.fractionLength(amount > 0 && amount < 0.01 ? 4 : 2)))
  }
}

/// Settings → AI: what the AI checks have cost, the limits that stop them and the prices used.
struct AIUsageSections: View {
  @AppStorage(AISettings.providerKey) private var providerID = ""
  @AppStorage(AISettings.fallbackKey) private var fallbackID = ""
  @State private var prices = AISettings.app.prices
  @State private var summary: UsageSummary?
  @State private var failure: String?
  @State private var version = 0

  private static let days: UInt32 = 30

  /// The providers runs can use: the main one and the fallback.
  private var used: [VisionProvider] {
    [providerID, fallbackID].compactMap(AISettings.provider).reduce(into: []) { if !$0.contains($1) { $0.append($1) } }
  }

  var body: some View {
    Section {
      spending
    } header: {
      Text("Spending")
    } footer: {
      Text("Counted on this Mac from every AI check of the app and the jevision tool, at the prices below.")
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
    }
    .task(id: version) { load() }
    if !used.isEmpty {
      Section {
        ForEach(used, id: \.id) { LimitRows(provider: $0) { version += 1 } }
      } header: {
        Text("Limits")
      } footer: {
        Text("When 80 percent of a limit is used, run reports warn. At the limit, AI checks with that provider stop with a message, or go to the fallback provider. Leave empty for no limit.")
          .font(.callout)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
      priceSection
    }
  }

  @ViewBuilder private var spending: some View {
    if let failure {
      Label(failure, systemImage: "exclamationmark.triangle")
        .foregroundStyle(.secondary)
    } else if let summary, !summary.providers.isEmpty || !summary.days.isEmpty {
      ForEach(summary.providers, id: \.provider) { spend in
        LabeledContent {
          VStack(alignment: .trailing, spacing: 2) {
            Text("Today: \(AIMoney.text(spend.today.cost))")
            Text("This month: \(AIMoney.text(spend.month.cost))")
            if spend.month.unpricedCalls > 0 {
              Text("\(spend.month.unpricedCalls) checks without a price")
                .font(.caption)
                .foregroundStyle(.orange)
            }
          }
          .monospacedDigit()
        } label: {
          Text(verbatim: Self.name(spend.provider))
          Text("\(spend.month.calls) checks this month")
        }
        .accessibilityElement(children: .combine)
      }
      SpendChart(days: summary.days, count: Int(Self.days))
    } else if summary != nil {
      Text("No AI checks yet.")
        .foregroundStyle(.secondary)
    } else {
      ProgressView().controlSize(.small)
    }
  }

  private var priceSection: some View {
    Section {
      Grid(alignment: .leading, horizontalSpacing: 10, verticalSpacing: 8) {
        GridRow {
          Text("Model")
          Text("Input")
          Text("Output")
          Color.clear.gridCellUnsizedAxes([.horizontal, .vertical])
        }
        .font(.callout)
        .foregroundStyle(.secondary)
        ForEach(used, id: \.id) { provider in
          GridRow {
            Text(verbatim: provider.name).font(.headline).gridCellColumns(4)
          }
          ForEach(prices.indices.filter { prices[$0].provider == provider.id }, id: \.self) { priceRow($0) }
        }
      }
      HStack {
        Menu("Add Price") {
          ForEach(used, id: \.id) { provider in
            Button(provider.name) { prices.append(Price(provider: provider.id, model: "", input: 0, output: 0)) }
          }
        }
        .fixedSize()
        Spacer()
        Button("Restore Public Prices") { prices = aiDefaultPrices() }
      }
    } header: {
      Text("Prices")
    } footer: {
      Text("US dollars per million tokens, the public prices JEVision knows. They can be changed: providers change prices, and an account can have its own. A model takes the price of the longest name it starts with; an empty name covers all of the provider’s models.")
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
    }
    .onChange(of: prices) {
      AISettings.save(prices: prices, in: .standard)
      version += 1
    }
  }

  private func priceRow(_ index: Int) -> some View {
    let model = prices[index].model.isEmpty ? String(localized: "All models") : prices[index].model
    return GridRow {
      TextField("Model", text: $prices[index].model, prompt: Text("All models"))
        .labelsHidden()
      TextField("Input", value: price($prices[index].input), format: .number)
        .labelsHidden()
        .accessibilityLabel(Text("Input price for \(model)"))
      TextField("Output", value: price($prices[index].output), format: .number)
        .labelsHidden()
        .accessibilityLabel(Text("Output price for \(model)"))
      Button("Remove Price", systemImage: "minus.circle", role: .destructive) { prices.remove(at: index) }
        .labelStyle(.iconOnly)
        .buttonStyle(.borderless)
        .accessibilityLabel(Text("Remove the price for \(model)"))
    }
    .textFieldStyle(.roundedBorder)
  }

  /// Prices can't be negative.
  private func price(_ value: Binding<Double>) -> Binding<Double> {
    Binding(get: { value.wrappedValue }, set: { value.wrappedValue = max(0, $0) })
  }

  private func load() {
    do {
      summary = try aiUsageSummary(settings: AISettings.app.usage, days: Self.days)
      failure = nil
    } catch AiUsageError.Unreadable(let reason) {
      failure = String(localized: "Can’t read the AI usage journal: \(reason)")
    } catch {
      failure = error.localizedDescription
    }
  }

  static func name(_ provider: String) -> String {
    AISettings.provider(provider)?.name ?? provider
  }
}

/// A provider's daily and monthly limits.
private struct LimitRows: View {
  let provider: VisionProvider
  let changed: () -> Void
  @AppStorage private var daily: Double?
  @AppStorage private var monthly: Double?

  init(provider: VisionProvider, changed: @escaping () -> Void) {
    self.provider = provider
    self.changed = changed
    _daily = AppStorage(AISettings.dailyLimitKey(provider.id))
    _monthly = AppStorage(AISettings.monthlyLimitKey(provider.id))
  }

  var body: some View {
    LabeledContent {
      HStack {
        field(Text("Per day"), $daily)
          .accessibilityLabel(Text("Daily limit for \(provider.name), US dollars"))
        field(Text("Per month"), $monthly)
          .accessibilityLabel(Text("Monthly limit for \(provider.name), US dollars"))
      }
    } label: {
      Text(verbatim: provider.name)
    }
  }

  private func field(_ prompt: Text, _ value: Binding<Double?>) -> some View {
    TextField(
      "Limit",
      value: Binding(get: { value.wrappedValue }, set: { value.wrappedValue = $0.map { max(0, $0) }; changed() }),
      format: .currency(code: "USD"), prompt: prompt
    )
    .labelsHidden()
    .textFieldStyle(.roundedBorder)
    .frame(width: 110)
  }
}

/// Spending per day over the last days, a bar per day split by provider.
private struct SpendChart: View {
  let days: [DaySpend]
  let count: Int

  private struct Bar: Identifiable {
    let day: Date
    let provider: String
    let cost: Double
    var id: String { "\(day.timeIntervalSince1970)\(provider)" }
  }

  private var bars: [Bar] {
    let style = Date.ISO8601FormatStyle(timeZone: .current).year().month().day()
    return days.compactMap { d in
      (try? Date(d.day, strategy: style)).map { Bar(day: $0, provider: AIUsageSections.name(d.provider), cost: d.spend.cost) }
    }
  }

  private var range: ClosedRange<Date> {
    let calendar = Calendar.current
    let today = calendar.startOfDay(for: .now)
    let first = calendar.date(byAdding: .day, value: 1 - count, to: today) ?? today
    return first...(calendar.date(byAdding: .day, value: 1, to: today) ?? today)
  }

  var body: some View {
    Chart(bars) { bar in
      BarMark(x: .value("Day", bar.day, unit: .day), y: .value("Spent", bar.cost))
        .foregroundStyle(by: .value("Provider", bar.provider))
        .accessibilityLabel(Text(verbatim: "\(bar.provider), \(bar.day.formatted(date: .abbreviated, time: .omitted))"))
        .accessibilityValue(Text(AIMoney.text(bar.cost)))
    }
    .chartXScale(domain: range)
    .chartYAxis {
      AxisMarks { value in
        AxisGridLine()
        AxisValueLabel { if let amount = value.as(Double.self) { Text(AIMoney.text(amount)) } }
      }
    }
    .frame(height: 160)
    .accessibilityLabel(Text("Spending per day over the last \(count) days"))
  }
}
