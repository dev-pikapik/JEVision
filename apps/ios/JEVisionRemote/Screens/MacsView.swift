import SwiftUI

/// Where a screen leads. Every screen past the Macs list works on one Mac and one project.
enum Route: Hashable {
  case projects(HostRecord)
  case tests(HostRecord, project: String, folder: String)
  case test(HostRecord, project: String, path: String)
  case run(HostRecord, project: String, run: String)
  case report(HostRecord, project: String, run: String)
  case runs(HostRecord, project: String)
}

/// Opens a screen on the Macs stack from code, for screens that open one after some work
/// rather than from a link. Pushing onto the same path keeps later links working.
struct PushAction {
  let action: (Route) -> Void
  func callAsFunction(_ route: Route) { action(route) }
}

extension EnvironmentValues {
  @Entry var push = PushAction { _ in }
}

/// The paired Macs, which of them can be reached, and the way into each.
struct MacsView: View {
  @Environment(AppModel.self) private var model
  @State private var path: [Route] = []
  @State private var showsSettings = false
  @State private var scanAfterSettings = false

  var body: some View {
    NavigationStack(path: $path) {
      List {
        if model.storageFailed {
          Label("This iPhone couldn’t save the list of paired Macs. They’ll be forgotten when JEVision quits.", systemImage: "exclamationmark.triangle")
            .foregroundStyle(.orange)
        }
        Section {
          ForEach(model.hosts) { host in
            NavigationLink(value: Route.projects(host)) {
              MacRow(host: host, connection: model.connections[host.id])
            }
          }
        } footer: {
          Text("JEVision must be open on the Mac, and the Mac awake.")
        }
      }
      .navigationTitle("Macs")
      .toolbar {
        ToolbarItem(placement: .topBarLeading) {
          Button("Settings", systemImage: "gearshape") { showsSettings = true }
        }
        ToolbarItem(placement: .topBarTrailing) {
          Button("Pair a Mac", systemImage: "plus") { model.isScanning = true }
        }
      }
      .refreshable { await model.connectAll() }
      .task { await model.connectAll() }
      .navigationDestination(for: Route.self) { route in
        switch route {
        case .projects(let host): ProjectsView(host: host)
        case .tests(let host, let project, let folder): TestsView(host: host, project: project, folder: folder)
        case .test(let host, let project, let path): TestDetailView(host: host, project: project, path: path)
        case .run(let host, let project, let run): LiveRunView(host: host, project: project, run: run)
        case .report(let host, let project, let run): ReportView(host: host, project: project, run: run)
        case .runs(let host, let project): RunsView(host: host, project: project)
        }
      }
    }
    .environment(\.push, PushAction { path.append($0) })
    .sheet(isPresented: $showsSettings, onDismiss: scanIfAsked) {
      SettingsView {
        scanAfterSettings = true
        showsSettings = false
      }
    }
  }

  private func scanIfAsked() {
    if scanAfterSettings {
      scanAfterSettings = false
      model.isScanning = true
    }
  }
}

private struct MacRow: View {
  let host: HostRecord
  let connection: AppModel.Connection?

  var body: some View {
    HStack(spacing: 14) {
      IconTile(symbol: "desktopcomputer", color: isOnline ? .blue : .gray, size: 40)
      VStack(alignment: .leading, spacing: 2) {
        Text(host.name).font(.headline)
        HStack(spacing: 6) {
          status.image
            .imageScale(.small)
            .foregroundStyle(isOnline ? AnyShapeStyle(.green) : AnyShapeStyle(.secondary))
          Text(status.text)
        }
        .font(.subheadline)
        .foregroundStyle(.secondary)
      }
    }
    .padding(.vertical, 4)
    .accessibilityElement(children: .combine)
  }

  private var isOnline: Bool {
    if case .online = connection { true } else { false }
  }

  private var status: (text: String, image: Image) {
    switch connection {
    case .online:
      (String(localized: "Online"), Image(systemName: "circle.fill"))
    case .connecting, nil:
      (String(localized: "Connecting…"), Image(systemName: "circle.dotted"))
    case .unreachable(.revoked):
      (String(localized: "Removed on the Mac. Pair again."), Image(systemName: "hand.raised"))
    case .unreachable(.macOffline):
      (String(localized: "Can’t reach it. It may be asleep or offline."), Image(systemName: "moon.zzz"))
    case .unreachable(let error):
      (error.title, Image(systemName: "exclamationmark.triangle"))
    }
  }
}
