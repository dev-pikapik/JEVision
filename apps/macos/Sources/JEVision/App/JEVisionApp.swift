import JEVisionCore
import JEVisionHands
import SwiftUI

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
  /// Asks before an AI assistant runs a test, whether or not Settings is open.
  @State private var assistantRequests: AssistantRequests
  @State private var updates = Updates()
  @NSApplicationDelegateAdaptor private var appDelegate: AppDelegate

  init() {
    AppLog.started()
    if CrashReports.isEnabled { CrashCollector.apply(enabled: true) }
    AppLanguage.shared.start()
    ActiveKeyboard.shared.start()
    Appearance.saved.apply()
    // Settings of apps that a run which crashed or was stopped cleared go back, for closed apps.
    Task.detached(priority: .utility) { putBackAppSettings() }
    let project = ProjectStore()
    let runs = RunCenter()
    let records = RecordCenter()
    project.activeWork = { ProjectWork.current(runs: runs, records: records) }
    _project = State(initialValue: project)
    _runs = State(initialValue: runs)
    _records = State(initialValue: records)
    _indicators = State(initialValue: ActivityIndicators(runs: runs, records: records))
    _remote = State(initialValue: RemoteAccess(runs: runs, project: project))
    let scheduleNotifier = ScheduleNotifier()
    scheduleNotifier.start()
    _scheduleNotifier = State(initialValue: scheduleNotifier)
    let assistantRequests = AssistantRequests()
    assistantRequests.start()
    _assistantRequests = State(initialValue: assistantRequests)
    appDelegate.project = project
  }

  var body: some Scene {
    Window("JEVision", id: SceneID.main) {
      MainView(appDelegate: appDelegate)
        .appLocale()
        .defaultAppStorage(Profile.current.defaults)
        .environment(project)
        .environment(permissions)
        .environment(runs)
        .environment(records)
        .environment(updates)
    }
    // Wide enough for the whole editor toolbar, Run and Record included, next to the sidebar.
    .defaultSize(width: 1200, height: 760)
    .commands { AppCommands(project: project, runs: runs, records: records, updates: updates) }

    Window("Welcome to JEVision", id: SceneID.onboarding) {
      OnboardingView()
        .appLocale()
        .defaultAppStorage(Profile.current.defaults)
        .environment(permissions)
        .environment(project)
        .environment(records)
        .environment(runs)
    }
    .windowResizability(.contentSize)
    .windowStyle(.hiddenTitleBar)
    .defaultPosition(.center)
    .restorationBehavior(.disabled)

    Window("Command Line Tool", id: SceneID.commandLineTool) {
      CommandLineToolView()
        .appLocale()
        .defaultAppStorage(Profile.current.defaults)
    }
    .windowResizability(.contentSize)
    .defaultPosition(.center)
    .restorationBehavior(.disabled)

    Window("Commit Changes", id: SceneID.commit) {
      CommitView()
        .appLocale()
        .defaultAppStorage(Profile.current.defaults)
        .environment(project)
    }
    .defaultSize(width: 960, height: 640)
    .restorationBehavior(.disabled)

    // A window of its own: as a sheet it is wider than the Settings window it would hang from.
    Window("Areas Always Hidden", id: SceneID.hiddenAreas) {
      ProjectHideEditor()
        .appLocale()
        .defaultAppStorage(Profile.current.defaults)
        .environment(project)
    }
    .defaultPosition(.center)
    .restorationBehavior(.disabled)

    Window("Statistics", id: SceneID.statistics) {
      StatisticsView()
        .appLocale()
        .defaultAppStorage(Profile.current.defaults)
        .environment(project)
    }
    .defaultSize(width: 960, height: 760)
    .restorationBehavior(.disabled)

    Window("Pull Requests", id: SceneID.pullRequests) {
      PullRequestsView()
        .appLocale()
        .defaultAppStorage(Profile.current.defaults)
        .environment(project)
    }
    .defaultSize(width: 1040, height: 680)
    .restorationBehavior(.disabled)

    Settings {
      SettingsView()
        .appLocale()
        .defaultAppStorage(Profile.current.defaults)
        .environment(project)
        .environment(permissions)
        .environment(remote)
        .environment(updates)
    }
    .windowResizability(.contentMinSize)
  }
}

enum SceneID {
  static let main = "main"
  static let onboarding = "onboarding"
  static let commandLineTool = "commandLineTool"
  static let commit = "commit"
  static let pullRequests = "pullRequests"
  static let statistics = "statistics"
  static let hiddenAreas = "hiddenAreas"
}
