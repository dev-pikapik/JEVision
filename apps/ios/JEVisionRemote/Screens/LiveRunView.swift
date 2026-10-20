import SwiftUI

/// A run as it happens on the Mac: the block running now, finished steps, and Stop.
struct LiveRunView: View {
  let host: HostRecord
  let project: String
  let run: String
  @Environment(AppModel.self) private var model
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @State private var progress = RunProgress()
  @State private var flow: TestFlow?
  @State private var lost: RemoteError?
  @State private var stopping = false
  @State private var stopError: RemoteError?
  @State private var attempt = 0

  var body: some View {
    ScrollViewReader { proxy in
      ScrollView {
        VStack(alignment: .leading, spacing: 16) {
          banner
          if let flow {
            FlowView(flow: flow, marks: progress.marks, current: progress.current, maxHeight: 420)
              .background(.fill.quinary, in: .rect(cornerRadius: 20))
          }
          if !progress.lines.isEmpty {
            VStack(alignment: .leading, spacing: 10) {
              Text("Steps").font(.headline)
              ForEach(progress.lines) { step in
                StepLine(step: step, kind: kind(of: step)).id(step.id)
              }
            }
          }
        }
        .padding()
      }
      // The newest step stays in view, scrolling no further than needed to show it.
      .onChange(of: progress.lines.last) { _, step in
        guard let step else { return }
        withAnimation(reduceMotion ? nil : .smooth) { proxy.scrollTo(step.id) }
      }
    }
    .navigationTitle(flow?.name ?? String(localized: "Run"))
    .navigationBarTitleDisplayMode(.inline)
    .safeAreaInset(edge: .bottom) { bottomBar.dynamicTypeSize(...DynamicTypeSize.accessibility2) }
    .alert(stopError?.title ?? "", isPresented: Binding(get: { stopError != nil }, set: { if !$0 { stopError = nil } })) {
      Button("OK", role: .cancel) {}
    } message: {
      Text(stopError?.message ?? "")
    }
    .task(id: attempt) { await follow() }
    .sensoryFeedback(trigger: progress.verdict) { _, verdict in
      switch verdict {
      case .passed: .success
      case .failed: .error
      default: nil
      }
    }
  }

  @ViewBuilder private var banner: some View {
    HStack(alignment: .top, spacing: 12) {
      bannerIcon
        .font(.title2)
        .frame(width: 30)
      VStack(alignment: .leading, spacing: 4) {
        Text(bannerTitle).font(.headline)
        if let detail = bannerDetail {
          Text(detail)
            .font(.subheadline)
            .foregroundStyle(.secondary)
        }
      }
      Spacer(minLength: 0)
    }
    .padding(16)
    .glassEffect(.regular, in: .rect(cornerRadius: 22))
    .accessibilityElement(children: .combine)
    .animation(.smooth, value: progress)
  }

  @ViewBuilder private var bannerIcon: some View {
    if lost != nil {
      Image(systemName: "wifi.slash").foregroundStyle(.orange)
    } else if let verdict = progress.verdict, progress.isFinished {
      VerdictStyle(verdict, flaky: progress.flaky).image
    } else if progress.state == .paused {
      Image(systemName: "pause.circle.fill").foregroundStyle(.orange)
    } else {
      ProgressView()
    }
  }

  private var bannerTitle: String {
    if let lost { return lost.title }
    if let verdict = progress.verdict, progress.isFinished { return VerdictStyle(verdict, flaky: progress.flaky).title }
    if progress.state == .paused { return String(localized: "Paused on the Mac") }
    if progress.attempt > 1 { return String(localized: "Running, try \(progress.attempt)") }
    return String(localized: "Running on \(host.name)")
  }

  private var bannerDetail: String? {
    if lost != nil { return String(localized: "The test goes on on the Mac. Reconnect to keep watching.") }
    if progress.isFinished {
      if progress.verdict == .failed, let failure = progress.failure { return RunText.failure(failure) }
      if progress.verdict == .cancelled { return String(localized: "The run was stopped.") }
      if progress.verdict == .unknown {
        return String(localized: "The Mac reported a result this version of the app doesn’t know. Update the app to see it.")
      }
      return nil
    }
    if let current = progress.current, let block = flow?.blocks.first(where: { $0.id == current }) {
      return block.title.isEmpty ? BlockInfo.title(block.kind) : "\(BlockInfo.title(block.kind)): \(block.title)"
    }
    return nil
  }

  /// A block that started before the phone was following the run only reports finishing, so
  /// its kind comes from the flow when the block is one of the run's own test.
  private func kind(of step: RunProgress.Step) -> String {
    guard step.kind.isEmpty, progress.test == nil || progress.test == step.test else { return step.kind }
    return flow?.blocks.first { $0.id == step.block }?.kind ?? ""
  }

  @ViewBuilder private var bottomBar: some View {
    Group {
      if lost != nil {
        Button { attempt += 1 } label: {
          Label("Reconnect", systemImage: "arrow.clockwise").frame(maxWidth: .infinity)
        }
        .buttonStyle(.glassProminent)
      } else if progress.isFinished {
        NavigationLink(value: Route.report(host, project: project, run: run)) {
          Label("Show Report", systemImage: "doc.text.magnifyingglass").frame(maxWidth: .infinity)
        }
        .buttonStyle(.glassProminent)
        .accessibilityIdentifier("showReport")
      } else if model.permission(on: host) == .run {
        Button(role: .destructive, action: stop) {
          Label(stopping ? "Stopping…" : "Stop", systemImage: "stop.fill").frame(maxWidth: .infinity)
        }
        .buttonStyle(.glass)
        .tint(.red)
        .disabled(stopping)
      }
    }
    .controlSize(.large)
    .padding(.horizontal, 24)
    .padding(.bottom, 8)
  }

  private func follow() async {
    lost = nil
    let session: any MacSession
    do {
      session = try await model.session(for: host)
    } catch {
      lost = error
      return
    }
    do {
      for try await update in session.runEvents(project: project, run: run) {
        progress.apply(update)
        if flow == nil, let test = progress.test {
          flow = try? await session.getTest(project: project, path: test)
        }
      }
    } catch is CancellationError {
    } catch {
      let error = error as? RemoteError ?? .other
      model.connectionFailed(host, error)
      if !progress.isFinished { lost = error }
    }
  }

  private func stop() {
    stopping = true
    let (project, run) = (project, run)
    Task {
      if case .failed(let error) = await model.fetch(host, { session throws(RemoteError) in
        try await session.stopRun(project: project, run: run)
      }) {
        stopError = error
      }
      stopping = false
    }
  }
}

private struct StepLine: View {
  let step: RunProgress.Step
  let kind: String

  var body: some View {
    HStack(alignment: .top, spacing: 10) {
      if let status = step.status {
        VerdictStyle(status).image
      } else {
        ProgressView().accessibilityLabel(Text("Running now"))
      }
      VStack(alignment: .leading, spacing: 2) {
        Text(kind.isEmpty ? step.block : BlockInfo.title(kind))
        if let took = step.took {
          Text(took)
            .font(.subheadline)
            .foregroundStyle(.secondary)
        }
        if let failure = step.failure {
          Text(RunText.failure(failure))
            .font(.subheadline)
            .foregroundStyle(.secondary)
        }
      }
      Spacer(minLength: 0)
    }
    .accessibilityElement(children: .combine)
  }
}
