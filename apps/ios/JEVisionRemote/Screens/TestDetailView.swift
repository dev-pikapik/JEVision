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
  @State private var startedRun: String?

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
      .safeAreaInset(edge: .bottom) { runBar }
    }
    .navigationTitle(flow.value?.name ?? "")
    .navigationBarTitleDisplayMode(.inline)
    .navigationDestination(item: $startedRun) { run in
      LiveRunView(host: host, project: project, run: run)
    }
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

  private func load() async {
    if flow.value == nil { flow = .loading }
    let (project, path) = (project, path)
    flow = await model.fetch(host) { session throws(RemoteError) in try await session.getTest(project: project, path: path) }
    if case .loaded(let list) = await model.fetch(host, { session throws(RemoteError) in try await session.listRuns(project: project) }) {
      runs = list.saved.filter { $0.file == path }.sorted { $0.startedAt > $1.startedAt }
    }
  }

  private func run() {
    starting = true
    let (project, path) = (project, path)
    Task {
      switch await model.fetch(host, { session throws(RemoteError) in try await session.runTest(project: project, path: path) }) {
      case .loaded(let run): startedRun = run
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
