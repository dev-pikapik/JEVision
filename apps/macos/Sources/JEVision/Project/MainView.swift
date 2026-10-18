import SwiftUI

struct MainView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(RunCenter.self) private var runs
  @Environment(RecordCenter.self) private var records
  @Environment(\.openWindow) private var openWindow
  @AppStorage(OnboardingView.doneKey) private var onboardingDone = false

  var body: some View {
    @Bindable var project = project
    @Bindable var runs = runs
    @Bindable var records = records
    Group {
      switch project.status {
      case .closed:
        EmptyStateView(
          symbol: "folder",
          title: Text("Open a project"),
          message: Text("Choose the folder that holds your tests. JEVision shows every .flow.json file in it."),
          action: Text("Open Project…"), perform: project.choose)
      case .unavailable:
        EmptyStateView(
          symbol: "questionmark.folder",
          title: Text("Can’t open “\(project.name)”"),
          message: Text("The folder was moved, deleted or is on a disk that isn’t connected. Choose it again to keep working."),
          action: Text("Open Project…"), perform: project.choose)
      case .loading:
        ProgressView()
          .accessibilityLabel(Text("Looking for tests"))
      case .ready where project.items.isEmpty:
        EmptyStateView(
          symbol: "doc.badge.plus",
          title: Text("No tests yet"),
          message: Text("A test is a chain of steps JEVision repeats in an app: open it, click, type, check how the screen looks."),
          action: Text("New Test…"), perform: { project.isCreatingTest = true })
      case .ready:
        ProjectView()
      }
    }
    .sheet(isPresented: $project.isCreatingTest) { NewTestSheet() }
    .sheet(item: $runs.blocked) { RunReadinessSheet(request: $0) }
    .sheet(item: $records.blocked) { RecordReadinessSheet(request: $0) }
    // The recording typed secrets that have no value yet: ask for each in turn.
    .sheet(item: Binding(
      get: { records.unsetSecrets.first.map(SecretName.init) },
      set: { if $0 == nil, !records.unsetSecrets.isEmpty { records.unsetSecrets.removeFirst() } }
    )) { secret in
      SetSecretSheet(name: secret.id) {}
    }
    .alert(
      "Recording not added", isPresented: Binding(get: { records.failure != nil }, set: { if !$0 { records.failure = nil } })
    ) {
      Button("OK") {}
    } message: {
      Text(records.failure ?? "")
    }
    .frame(minWidth: 640, minHeight: 420)
    .navigationTitle(project.rootURL == nil ? "JEVision" : project.name)
    .onAppear {
      if !onboardingDone { openWindow(id: SceneID.onboarding) }
    }
  }
}
