import SwiftUI

/// One test: its flow, its last results, and the Run button.
struct TestDetailView: View {
  let host: HostRecord
  let project: String
  let path: String
  @Environment(AppModel.self) private var model
  @State private var flow: Loadable<TestFlow> = .loading
  @State private var runs: [RunSummary] = []
  @State private var starting = false
  @State private var runError: RemoteError?
  @Environment(\.push) private var push
  /// Row of the data table to run, counted from 0; `nil` runs every row.
  @State private var row: Int?

  var body: some View {
    LoadableView(state: flow, retry: load) { flow in
      ScrollView {
        VStack(alignment: .leading, spacing: 20) {
          if let description = flow.description, !description.isEmpty {
            Text(description)
              .foregroundStyle(.secondary)
              .padding(.horizontal)
          }
          FlowView(flow: flow, maxHeight: 520)
            .background(.fill.quinary, in: .rect(cornerRadius: 20))
            .padding(.horizontal)
          if !runs.isEmpty {
            VStack(alignment: .leading, spacing: 0) {
              Text("Last Runs")
                .font(.headline)
                .padding(.horizontal)
                .padding(.bottom, 8)
              ForEach(runs.prefix(5)) { run in
                NavigationLink(value: Route.report(host, project: project, run: run.run)) {
                  HStack {
                    RunRow(run: run, showsTest: false)
                    Image(systemName: "chevron.right")
                      .font(.footnote.weight(.semibold))
                      .foregroundStyle(.tertiary)
                      .accessibilityHidden(true)
                  }
                  .padding(.horizontal)
                    .padding(.vertical, 10)
                }
                .buttonStyle(.plain)
                Divider().padding(.leading)
              }
            }
          }
        }
        .padding(.vertical)
      }
      .refreshable { await load() }
      .safeAreaInset(edge: .bottom) { runBar.dynamicTypeSize(...DynamicTypeSize.accessibility2) }
    }
    .navigationTitle(flow.value?.name ?? "")
    .navigationBarTitleDisplayMode(.inline)
    .alert(runError?.title ?? "", isPresented: Binding(get: { runError != nil }, set: { if !$0 { runError = nil } })) {
      Button("OK", role: .cancel) {}
    } message: {
      Text(runError?.message ?? "")
    }
    .task { await load() }
  }

  @ViewBuilder private var runBar: some View {
    switch model.permission(on: host) {
    case .run:
      VStack(spacing: 10) {
        if let rows = flow.value?.rows, !rows.isEmpty {
          rowMenu(rows)
        }
        runButton
      }
      .padding(.horizontal, 24)
      .padding(.bottom, 8)
    case .view:
      Label("This iPhone can only look at tests. Allow runs for it in JEVision on the Mac.", systemImage: "eye")
        .font(.footnote)
        .foregroundStyle(.secondary)
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
        .glassEffect(.regular, in: .rect(cornerRadius: 18))
        .padding(.horizontal, 16)
        .padding(.bottom, 8)
    case nil:
      EmptyView()
    }
  }

  private var runButton: some View {
    Button(action: run) {
      Group {
        if starting {
          HStack(spacing: 10) {
            ProgressView()
            Text("Waiting for the Mac…")
          }
        } else {
          Label("Run on \(host.name)", systemImage: "play.fill")
        }
      }
      .frame(maxWidth: .infinity)
    }
    .buttonStyle(.glassProminent)
    .controlSize(.large)
    .disabled(starting)
    .accessibilityIdentifier("run")
  }

  /// Picks the data row to run: the test runs once per row unless one is chosen.
  private func rowMenu(_ rows: [String]) -> some View {
    Menu {
      Picker("Data Rows", selection: $row) {
        Text("All Rows").tag(Int?.none)
        ForEach(rows.indices, id: \.self) { index in
          Text(Self.rowTitle(number: index + 1, label: rows[index])).tag(Int?.some(index))
        }
      }
    } label: {
      Label {
        Text(row.map { Self.rowTitle(number: $0 + 1, label: rows[$0]) } ?? String(localized: "Data Rows: All \(rows.count)"))
          .lineLimit(1)
      } icon: {
        Image(systemName: "tablecells")
      }
      .frame(maxWidth: .infinity)
    }
    .buttonStyle(.glass)
    .controlSize(.large)
    .disabled(starting)
    .accessibilityLabel("Data Rows")
    .accessibilityIdentifier("rowMenu")
    .accessibilityValue(row.map { Self.rowTitle(number: $0 + 1, label: rows[$0]) } ?? String(localized: "All Rows"))
  }

  /// “Row 2: anna”, from the row's `name` column or else its first, as the Mac names it.
  nonisolated static func rowTitle(number: Int, label: String) -> String {
    label.isEmpty ? String(localized: "Row \(number)") : String(localized: "Row \(number): \(label)")
  }

  private func load() async {
    if flow.value == nil { flow = .loading }
    let (project, path) = (project, path)
    flow = await model.fetch(host) { session throws(RemoteError) in try await session.getTest(project: project, path: path) }
    if let row, row >= flow.value?.rows.count ?? 0 { self.row = nil }
    if case .loaded(let list) = await model.fetch(host, { session throws(RemoteError) in try await session.listRuns(project: project) }) {
      runs = list.saved.filter { $0.file == path }.sorted { $0.startedAt > $1.startedAt }
    }
  }

  private func run() {
    starting = true
    let (project, path, row) = (project, path, row)
    Task {
      switch await model.fetch(host, { session throws(RemoteError) in
        try await session.runTest(project: project, path: path, row: row)
      }) {
      case .loaded(let run): push(.run(host, project: project, run: run))
      case .failed(let error): runError = error
      case .loading: break
      }
      starting = false
    }
  }
}

/// A saved run in a list: result, when, how long.
struct RunRow: View {
  let run: RunSummary
  let showsTest: Bool

  var body: some View {
    let style = VerdictStyle(run.verdict, flaky: run.flaky)
    HStack(spacing: 12) {
      style.image.font(.title3)
      VStack(alignment: .leading, spacing: 2) {
        Text(showsTest ? run.test : style.title)
          .foregroundStyle(.primary)
        Text("\(run.startedAt, format: .dateTime.day().month().hour().minute()) · \(Duration.milliseconds(run.durationMs), format: .units(allowed: [.minutes, .seconds], width: .abbreviated))")
          .font(.subheadline)
          .foregroundStyle(.secondary)
      }
      Spacer(minLength: 0)
    }
    .contentShape(.rect)
    .accessibilityElement(children: .combine)
  }
}
