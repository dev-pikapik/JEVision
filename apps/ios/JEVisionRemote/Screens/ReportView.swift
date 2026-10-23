import SwiftUI

/// A finished run: how it ended in one phrase, then every step. Pictures come from the Mac
/// only when a step is opened.
struct ReportView: View {
  let host: HostRecord
  let project: String
  let run: String
  @Environment(AppModel.self) private var model
  @State private var report: Loadable<RunReport> = .loading

  var body: some View {
    LoadableView(state: report, retry: load) { report in
      List {
        Section {
          ReportHeader(report: report)
        }
        ForEach(Array(report.attempts.enumerated()).reversed(), id: \.offset) { _, attempt in
          Section {
            ForEach(attempt.steps) { step in
              StepRow(step: step) { name in
                await model.fetch(host) { session throws(RemoteError) in
                  try await session.picture(project: project, run: run, name: name)
                }
              }
            }
          } header: {
            if let title = Self.title(of: attempt, in: report) {
              Text(title)
            }
          }
        }
      }
      .navigationTitle(report.test)
    }
    .navigationBarTitleDisplayMode(.inline)
    .task { await load() }
  }

  /// `Row 2: Anna`, `Try 2`, or both when a row was retried; `nil` for a run with one try.
  nonisolated static func title(of attempt: RunReport.Attempt, in report: RunReport) -> String? {
    let row = attempt.row.map { TestDetailView.rowTitle(number: $0.number, label: $0.label) }
    let retried = report.attempts.contains { $0.number > 1 }
    switch (row, retried) {
    case (let row?, true): return String(localized: "\(row) · Try \(attempt.number)")
    case (let row?, false): return row
    case (nil, _) where report.attempts.count > 1: return String(localized: "Try \(attempt.number)")
    case (nil, _): return nil
    }
  }

  private func load() async {
    if report.value == nil { report = .loading }
    let (project, run) = (project, run)
    report = await model.fetch(host) { session throws(RemoteError) in try await session.report(project: project, run: run) }
  }
}

private struct ReportHeader: View {
  let report: RunReport

  var body: some View {
    let style = VerdictStyle(report.verdict, flaky: report.flaky)
    VStack(alignment: .leading, spacing: 8) {
      HStack(spacing: 10) {
        style.image.font(.title)
        Text(style.title).font(.title2.bold())
      }
      Text(RunText.outcome(report))
      Text("\(report.startedAt, format: .dateTime.day().month().year().hour().minute()) · \(report.file)")
        .font(.footnote)
        .foregroundStyle(.secondary)
      if let device = report.requestedBy {
        Text("Started from \(device)")
          .font(.footnote)
          .foregroundStyle(.secondary)
      }
    }
    .padding(.vertical, 4)
    .accessibilityElement(children: .combine)
  }
}

private struct StepRow: View {
  let step: RunReport.Step
  let loadPicture: (String) async -> Loadable<Data>
  @State private var expanded = false

  private var pictures: [(title: String, name: String)] {
    [
      (String(localized: "Screen After"), step.after),
      (String(localized: "Expected"), step.expected),
      (String(localized: "Difference"), step.diff),
      (String(localized: "Screen Before"), step.before),
    ].compactMap { title, name in name.map { (title, $0) } }
  }

  var body: some View {
    if pictures.isEmpty {
      summary
    } else {
      DisclosureGroup(isExpanded: $expanded) {
        ForEach(pictures, id: \.name) { picture in
          RemotePicture(title: picture.title, name: picture.name, load: loadPicture)
        }
      } label: {
        summary
      }
    }
  }

  private var summary: some View {
    let style = VerdictStyle(step.status)
    return HStack(alignment: .top, spacing: 10) {
      style.image
      VStack(alignment: .leading, spacing: 2) {
        Text(BlockInfo.title(step.kind))
        if let branch = RunText.branch(step) {
          Text(branch)
            .font(.subheadline)
            .foregroundStyle(.secondary)
        }
        if let failure = step.failure {
          Text(RunText.failure(failure))
            .font(.subheadline)
            .foregroundStyle(.secondary)
        }
        if let note = step.note {
          Label(RunText.failure(note), systemImage: "info.circle")
            .font(.subheadline)
            .foregroundStyle(.secondary)
        }
      }
      Spacer(minLength: 0)
      Text(Duration.milliseconds(step.durationMs), format: .units(allowed: [.minutes, .seconds, .milliseconds], width: .narrow, maximumUnitCount: 1))
        .font(.footnote.monospacedDigit())
        .foregroundStyle(.secondary)
    }
    .accessibilityElement(children: .combine)
  }
}

/// One of a step's pictures, fetched from the Mac when it comes into view.
private struct RemotePicture: View {
  let title: String
  let name: String
  let load: (String) async -> Loadable<Data>
  @State private var state: Loadable<UIImage> = .loading

  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      Text(title)
        .font(.footnote.weight(.semibold))
        .foregroundStyle(.secondary)
      switch state {
      case .loading:
        ProgressView()
          .frame(maxWidth: .infinity, minHeight: 120)
      case .loaded(let image):
        Image(uiImage: image)
          .resizable()
          .scaledToFit()
          .clipShape(.rect(cornerRadius: 10))
          .accessibilityLabel(title)
      case .failed(let error):
        Label(error.title, systemImage: error.symbol)
          .font(.footnote)
          .foregroundStyle(.secondary)
          .frame(maxWidth: .infinity, minHeight: 60)
          .onTapGesture { Task { await fetch() } }
      }
    }
    .padding(.vertical, 4)
    .task { await fetch() }
  }

  private func fetch() async {
    state = .loading
    switch await load(name) {
    case .loaded(let data):
      state = UIImage(data: data).map(Loadable.loaded) ?? .failed(.other)
    case .failed(let error):
      state = .failed(error)
    case .loading:
      break
    }
  }
}

#Preview {
  List {
    ReportHeader(
      report: RunReport(
        test: "Sign in", file: "sign-in.json", app: "com.example.Notes", startedAt: .now, durationMs: 8_400,
        verdict: .failed, flaky: false, baselinesCreated: [],
        attempts: [
          .init(
            number: 1, verdict: .failed, durationMs: 8_400,
            failure: Failure(code: "run.target.notFound", params: ["block": "Click", "image": "save.png"]), steps: [])
        ]))
  }
}
