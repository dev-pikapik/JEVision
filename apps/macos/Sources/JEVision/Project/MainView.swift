import SwiftUI

struct MainView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(RunCenter.self) private var runs
  @Environment(RecordCenter.self) private var records
  @Environment(\.openWindow) private var openWindow
  @Environment(\.openSettings) private var openSettings
  @Environment(\.undoManager) private var undoManager
  @AppStorage(OnboardingView.doneKey) private var onboardingDone = false
  let appDelegate: AppDelegate
  /// The New Test or New Project sheet was opened to record: the test it creates is recorded
  /// once it closes.
  @State private var recordsNewTest = false
  @State private var createdTestToRecord = false

  var body: some View {
    @Bindable var project = project
    @Bindable var runs = runs
    @Bindable var records = records
    @Bindable var schedules = project.schedules
    @Bindable var git = project.git
    ProjectView(recordFirstTest: {
      recordsNewTest = true
      project.isCreatingTest = true
    })
    .modifier(WorkspaceChrome())
    .sheet(isPresented: $project.isCreatingTest, onDismiss: recordCreatedTest) {
      NewTestSheet(records: recordsNewTest) { createdTestToRecord = recordsNewTest }
    }
    .sheet(item: $project.newProject, onDismiss: recordCreatedTest) { request in
      NewProjectSheet(request: request) { (recordsNewTest, createdTestToRecord) = (true, true) }
    }
    .sheet(isPresented: Binding(get: { project.historyPath != nil }, set: { if !$0 { project.historyPath = nil } })) {
      if let path = project.historyPath { HistoryView(path: path, undoManager: undoManager) }
    }
    .sheet(isPresented: Binding(get: { project.bugReportRun != nil }, set: { if !$0 { project.bugReportRun = nil } })) {
      if let dir = project.bugReportRun { BugReportSheet(dir: dir) }
    }
    .sheet(item: $project.installingBlock) { InstallBlockSheet(model: $0) }
    .sheet(isPresented: $project.showsCustomBlocks) { CustomBlocksSheet() }
    .sheet(item: $project.owningBlock) { OwnBlockSheet(model: $0) }
    .dropDestination(for: URL.self) { urls, _ in
      if project.status == .ready, let source = urls.lazy.compactMap(BlockInstallModel.source).first {
        project.installBlock(from: source)
        return true
      }
      // A project folder or one of its files opens that project.
      return urls.contains { project.openItem($0) }
    }
    .sheet(item: $project.referenceWizard) { ReferenceWizardView(wizard: $0, undoManager: undoManager) }
    .sheet(isPresented: $schedules.isShowingSheet) { SchedulesSheet() }
    .sheet(isPresented: $git.isComposingPullRequest) { NewPullRequestView() }
    .sheet(isPresented: Binding(get: { git.conflicts != nil }, set: { if !$0 { git.conflicts = nil } })) {
      if let paths = git.conflicts { ConflictsView(paths: paths) }
    }
    .sheet(item: $runs.blocked) { RunReadinessSheet(request: $0) }
    .sheet(item: $records.blocked) { RecordReadinessSheet(request: $0) }
    // The recording typed secrets that have no value yet: ask for each in turn.
    .sheet(item: Binding(
      get: { records.unsetSecrets.first.map(SecretName.init) },
      set: { if $0 == nil, !records.unsetSecrets.isEmpty { records.unsetSecrets.removeFirst() } }
    )) { secret in
      ProjectSecretSheet(name: secret.id, project: records.secretsProject, environment: nil) {}
    }
    .alert(
      "Recording not added", isPresented: Binding(get: { records.failure != nil }, set: { if !$0 { records.failure = nil } })
    ) {
      Button("OK") {}
    } message: {
      Text(records.failure ?? "")
    }
    .background {
      CommandPalettePresenter(isPresented: project.showsCommandPalette) {
        project.showsCommandPalette = false
      } palette: {
        CommandPaletteView(openWindow: openWindow, openSettings: openSettings)
          .environment(project)
          .environment(runs)
          .environment(records)
      }
    }
    .frame(minWidth: 640, minHeight: 420)
    .navigationTitle(project.rootURL == nil ? Text("Projects") : Text(verbatim: project.name))
    .onAppear {
      appDelegate.showMainWindow = { openWindow(id: SceneID.main) }
      if !onboardingDone { openWindow(id: SceneID.onboarding) }
    }
  }

  private func recordCreatedTest() {
    defer { (recordsNewTest, createdTestToRecord) = (false, false) }
    guard createdTestToRecord, let document = project.document else { return }
    records.record(document, in: project.rootURL)
  }
}
