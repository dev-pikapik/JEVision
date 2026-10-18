import SwiftUI

@main
struct JEVisionApp: App {
  @State private var project = ProjectStore()
  @State private var permissions = PermissionsMonitor()
  @State private var runs = RunCenter()

  init() {
    _ = AppLanguage.atLaunch
    Appearance.saved.apply()
  }

  var body: some Scene {
    Window("JEVision", id: SceneID.main) {
      MainView()
        .environment(project)
        .environment(permissions)
        .environment(runs)
    }
    .defaultSize(width: 960, height: 640)
    .commands { AppCommands(project: project, runs: runs) }

    Window("Welcome to JEVision", id: SceneID.onboarding) {
      OnboardingView()
        .environment(permissions)
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

    Settings {
      SettingsView()
        .environment(permissions)
    }
  }
}

enum SceneID {
  static let main = "main"
  static let onboarding = "onboarding"
  static let commandLineTool = "commandLineTool"
}
