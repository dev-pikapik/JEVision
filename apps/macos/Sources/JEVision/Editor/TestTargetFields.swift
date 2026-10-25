import AppKit
import JEVisionCore
import SwiftUI
import UniformTypeIdentifiers

/// Where a test runs, this Mac or a simulated iPhone or iPad, and the app it tests there.
struct TestTargetFields: View {
  let document: FlowDocument
  let flow: Flow
  let issues: [Issue]
  @State private var devices: [SimulatedDevice] = []
  @State private var devicesError: String?

  var body: some View {
    Picker(selection: deviceBinding) {
      Text("This Mac").tag(String?.none)
      if !deviceNames.isEmpty { Divider() }
      ForEach(deviceNames, id: \.self) { Text(verbatim: $0).tag(String?.some($0)) }
    } label: {
      Text("Runs On")
      Text("A simulated iPhone or iPad runs in Simulator, which comes with Xcode.")
    }
    .task { await loadDevices() }
    if let device = flow.simulator?.device {
      if let devicesError {
        Text(devicesError).foregroundStyle(.secondary)
      }
      IssueList(issues: fieldIssues("simulator.device"))
      SimulatedAppField(
        device: device, value: flow.app, prompt: Text(verbatim: "com.example.App"),
        missing: Text("Not on \(device) yet. Give a build to install.")
      ) { app in document.updateFlow(field: "app") { $0.app = app } }
      IssueList(issues: fieldIssues("app"))
      LabeledContent {
        HStack {
          CommitField(title: "Build", prompt: Text(verbatim: "build/App.app"), value: flow.simulator?.install ?? "") { path in
            document.updateFlow(field: "simulator") { $0.simulator?.install = path.isEmpty ? nil : path }
          }
          .labelsHidden()
          Button("Choose…", action: chooseBuild)
        }
      } label: {
        Text("Build")
        Text("A built .app to install on the device before the test, relative to the test file. Open App with Start Fresh installs it again.")
      }
      IssueList(issues: fieldIssues("simulator.install"))
    } else {
      LabeledContent("App") {
        AppPicker(
          bundleID: Binding(
            get: { flow.app.isEmpty ? nil : flow.app },
            set: { new in document.updateFlow(field: "app") { $0.app = new ?? "" } }),
          noneTitle: Text("Choose App"))
      }
      IssueList(issues: fieldIssues("app"))
    }
  }

  /// One entry per device name, as the test names its device: the run picks the running one, or
  /// the one with the newest iOS. The device the test names stays listed when this Mac lacks it.
  private var deviceNames: [String] {
    var names: [String] = []
    for name in devices.map(\.name) + [flow.simulator?.device].compactMap(\.self) where !names.contains(name) {
      names.append(name)
    }
    return names
  }

  private var deviceBinding: Binding<String?> {
    let document = document
    return Binding(
      get: { flow.simulator?.device },
      set: { device in
        document.updateFlow(field: "simulator") { flow in
          if let device {
            flow.simulator = Simulator(device: device, install: flow.simulator?.install)
          } else {
            flow.simulator = nil
          }
        }
      })
  }

  private func fieldIssues(_ field: String) -> [Issue] {
    issues.filter { $0.param("field") == field }
  }

  private func loadDevices() async {
    let result = await Task.detached { Result { try simulatedDevices() } }.value
    switch result {
    case .success(let found):
      devices = found
      devicesError = nil
    case .failure(let error):
      devices = []
      devicesError = error.localizedDescription
    }
  }

  private func chooseBuild() {
    let panel = NSOpenPanel()
    panel.canChooseDirectories = false
    panel.canChooseFiles = true
    panel.allowsMultipleSelection = false
    panel.treatsFilePackagesAsDirectories = false
    panel.allowedContentTypes = [.applicationBundle]
    panel.message = String(localized: "Choose an app built for Simulator")
    panel.prompt = String(localized: "Choose")
    guard panel.runModal() == .OK, let url = panel.url else { return }
    let path = Self.relativePath(of: url, from: document.url.deletingLastPathComponent())
    let bundleID = Bundle(url: url)?.bundleIdentifier
    document.updateFlow(field: "simulator") { flow in
      flow.simulator?.install = path
      if let bundleID { flow.app = bundleID }
    }
  }

  /// `file` as a path from `folder`, such as `../build/App.app`.
  static func relativePath(of file: URL, from folder: URL) -> String {
    let to = file.standardizedFileURL.pathComponents
    let from = folder.standardizedFileURL.pathComponents
    let shared = zip(to, from).prefix { $0 == $1 }.count
    return (Array(repeating: "..", count: from.count - shared) + to.dropFirst(shared)).joined(separator: "/")
  }
}

/// The bundle id of an app on a simulated device: typed, or chosen from the apps on it.
struct SimulatedAppField: View {
  let device: String
  let value: String
  let prompt: Text
  /// Says the app isn't on the device.
  let missing: Text
  let save: (String) -> Void
  @State private var apps: [SimulatedApp] = []

  var body: some View {
    LabeledContent {
      HStack {
        CommitField(title: "App", prompt: prompt, value: value, save: save)
          .labelsHidden()
        Menu("Choose") {
          let user = apps.filter(\.user)
          let system = apps.filter { !$0.user }
          ForEach(user, id: \.bundleId) { app in Button(app.name) { save(app.bundleId) } }
          if !user.isEmpty, !system.isEmpty { Divider() }
          ForEach(system, id: \.bundleId) { app in Button(app.name) { save(app.bundleId) } }
        }
        .fixedSize()
        .disabled(apps.isEmpty)
      }
    } label: {
      Text("App")
      if let name = apps.first(where: { $0.bundleId == value })?.name {
        Text(verbatim: name)
      } else if !value.isEmpty, !apps.isEmpty {
        missing
      }
    }
    .task(id: device) {
      apps = (try? await Task.detached { [device] in try simulatedApps(device: device) }.value) ?? []
    }
  }
}
