import SwiftUI

/// A project's runs: those going on now, then the saved ones, newest first.
struct RunsView: View {
  let host: HostRecord
  let project: String
  @Environment(AppModel.self) private var model
  @State private var runs: Loadable<RunList> = .loading

  var body: some View {
    LoadableView(state: runs, retry: load) { runs in
      if runs.active.isEmpty && runs.saved.isEmpty {
        ContentUnavailableView(
          "No Runs Yet", systemImage: "clock",
          description: Text("Runs of this project’s tests, from the Mac or this iPhone, show up here."))
      } else {
        List {
          if !runs.active.isEmpty {
            Section("Running Now") {
              ForEach(runs.active, id: \.run) { status in
                NavigationLink(value: Route.run(host, project: project, run: status.run)) {
                  Label {
                    VStack(alignment: .leading, spacing: 2) {
                      Text(status.test)
                      if status.state == .unknown {
                        Text(VerdictStyle(Verdict.unknown).title)
                          .font(.subheadline)
                          .foregroundStyle(.secondary)
                      } else if status.waiting != nil {
                        Text("Waiting for the screen")
                          .font(.subheadline)
                          .foregroundStyle(.secondary)
                      }
                    }
                  } icon: {
                    if status.state == .unknown {
                      VerdictStyle(Verdict.unknown).image
                    } else {
                      Image(systemName: status.waiting != nil ? "clock.fill" : status.state == .paused ? "pause.circle.fill" : "play.circle.fill")
                        .foregroundStyle(.orange)
                    }
                  }
                }
              }
            }
          }
          Section("Recent") {
            ForEach(runs.saved.sorted { $0.startedAt > $1.startedAt }) { run in
              NavigationLink(value: Route.report(host, project: project, run: run.run)) {
                RunRow(run: run, showsTest: true)
              }
            }
          }
        }
        .refreshable { await load() }
      }
    }
    .navigationTitle("Runs")
    .task { await load() }
  }

  private func load() async {
    if runs.value == nil { runs = .loading }
    let project = project
    runs = await model.fetch(host) { session throws(RemoteError) in try await session.listRuns(project: project) }
  }
}
