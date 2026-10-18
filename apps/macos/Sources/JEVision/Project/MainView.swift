import SwiftUI

struct MainView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(RunCenter.self) private var runs
  @Environment(RecordCenter.self) private var records
  @Environment(\.openWindow) private var openWindow
  @Environment(\.undoManager) private var undoManager
  @AppStorage(OnboardingView.doneKey) private var onboardingDone = false
  /// The New Test sheet was opened to record: the test it creates is recorded once it closes.
  @State private var recordsNewTest = false
  @State private var createdTestToRecord = false

  var body: some View {
    @Bindable var project = project
    @Bindable var runs = runs
    @Bindable var records = records
    @Bindable var schedules = project.schedules
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
          symbol: "record.circle",
          title: Text("No tests yet"),
          message: Text("Record what you do in an app — JEVision turns every step into a block. Or start with an empty test and add the steps yourself."),
          action: Text("Record First Test…"),
          perform: {
            recordsNewTest = true
            project.isCreatingTest = true
          },
          secondAction: Text("New Test…"), performSecond: { project.isCreatingTest = true })
      case .ready:
        ProjectView()
      }
    }
    .sheet(isPresented: $project.isCreatingTest, onDismiss: recordCreatedTest) {
      NewTestSheet(records: recordsNewTest) { createdTestToRecord = recordsNewTest }
    }
    .sheet(isPresented: Binding(get: { project.historyPath != nil }, set: { if !$0 { project.historyPath = nil } })) {
      if let path = project.historyPath { HistoryView(path: path, undoManager: undoManager) }
    }
    .sheet(isPresented: $schedules.isShowingSheet) { SchedulesSheet() }
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

  private func recordCreatedTest() {
    defer { (recordsNewTest, createdTestToRecord) = (false, false) }
    guard createdTestToRecord, let document = project.document else { return }
    records.record(document)
  }
}
