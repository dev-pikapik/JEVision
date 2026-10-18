import SwiftUI

@main
struct JEVisionApp: App {
  @State private var project: ProjectStore
  @State private var permissions = PermissionsMonitor()
  @State private var runs: RunCenter
  @State private var records: RecordCenter
  /// The edge glow and the menu bar item while a test runs or records.
  @State private var indicators: ActivityIndicators
  /// Made at launch so paired iPhones reach the Mac whether or not Settings is open.
  @State private var remote: RemoteAccess
  /// Tells about failed scheduled runs, of every project with schedules.
  @State private var scheduleNotifier: ScheduleNotifier

  init() {
    _ = AppLanguage.atLaunch
    Appearance.saved.apply()
    let project = ProjectStore()
    let runs = RunCenter()
    let records = RecordCenter()
    _project = State(initialValue: project)
    _runs = State(initialValue: runs)
    _records = State(initialValue: records)
    _indicators = State(initialValue: ActivityIndicators(runs: runs, records: records))
    _remote = State(initialValue: RemoteAccess(runs: runs, project: project))
    let scheduleNotifier = ScheduleNotifier()
    scheduleNotifier.start()
    _scheduleNotifier = State(initialValue: scheduleNotifier)
  }

  var body: some Scene {
    Window("JEVision", id: SceneID.main) {
      MainView()
        .environment(project)
        .environment(permissions)
        .environment(runs)
        .environment(records)
    }
    .defaultSize(width: 960, height: 640)
    .commands { AppCommands(project: project, runs: runs, records: records) }

    Window("Welcome to JEVision", id: SceneID.onboarding) {
      OnboardingView()
        .environment(permissions)
        .environment(project)
        .environment(records)
    }
    .windowResizability(.contentSize)
    .windowStyle(.hiddenTitleBar)
    .defaultPosition(.center)
    .restorationBehavior(.disabled)

    Window("Command Line Tool", id: SceneID.commandLineTool) {
      CommandLineToolView()
    }
    .windowResizability(.contentSize)
    .defaultPosition(.center)
    .restorationBehavior(.disabled)

    Window("Commit Changes", id: SceneID.commit) {
      CommitView()
        .environment(project)
    }
    .defaultSize(width: 960, height: 640)
    .restorationBehavior(.disabled)

    Window("Pull Requests", id: SceneID.pullRequests) {
      PullRequestsView()
        .environment(project)
    }
    .defaultSize(width: 1040, height: 680)
    .restorationBehavior(.disabled)

    Settings {
      SettingsView()
        .environment(project)
        .environment(permissions)
        .environment(remote)
    }
  }
}

enum SceneID {
  static let main = "main"
  static let onboarding = "onboarding"
  static let commandLineTool = "commandLineTool"
  static let commit = "commit"
  static let pullRequests = "pullRequests"
}
