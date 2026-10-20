import AppKit
import SwiftUI

/// Settings → Advanced → Logs: how much the app log says, and where it is.
struct LogsSection: View {
  @Binding var level: String

  var body: some View {
    Section {
      Picker("Log detail", selection: $level) {
        ForEach(LogLevel.allCases, id: \.rawValue) { Text(Self.title($0)).tag($0.rawValue) }
      }
      .settingAnchor("logLevel")
      LabeledContent {
        Button("Show in Finder") {
          try? FileManager.default.createDirectory(at: AppLog.folder, withIntermediateDirectories: true)
          NSWorkspace.shared.open(AppLog.folder)
        }
      } label: {
        RowLabel(Text("Logs"), Text("In the Logs folder of your Library"))
      }
      .settingAnchor("showLogs")
    } header: {
      Text("Logs")
    } footer: {
      Text("JEVision writes what it does to a log on this Mac, to read or to attach to a bug report. Normal notes when a project opens and when a run starts and ends; Detailed adds every step of a run. Secrets and typed text are never written, and old lines are deleted so the log stays small. Nothing is sent anywhere.")
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
    }
  }

  static func title(_ level: LogLevel) -> String {
    switch level {
    case .error: String(localized: "Errors only")
    case .info: String(localized: "Normal")
    case .debug: String(localized: "Detailed")
    }
  }
}

/// Settings → Advanced → Crash Reports: off until the person turns it on.
struct CrashReportsSection: View {
  @Binding var isOn: Bool

  var body: some View {
    Section {
      Toggle("Save crash reports", isOn: $isOn)
        .settingAnchor("crashReports")
      LabeledContent {
        Button("Show in Finder") {
          let folder = CrashReports().folder
          try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
          NSWorkspace.shared.open(folder)
        }
      } label: {
        RowLabel(Text("Saved crash reports"), Text("In JEVision’s folder in Application Support"))
      }
    } header: {
      Text("Crash Reports")
    } footer: {
      Text("If JEVision crashes, macOS hands it a report the next time it starts, up to a day later, and JEVision saves the report on this Mac. Nothing is sent anywhere. You decide for each bug report whether to add one: the bug report window offers it.")
        .font(.callout)
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
    }
    .onChange(of: isOn) {
      CrashCollector.apply(enabled: isOn)
      AppLog.info("Crash reports are \(isOn ? "saved" : "not saved")")
    }
  }
}

/// Settings → Advanced → Learn: the example project and the guide to virtual machines.
struct GuidesSection: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.openWindow) private var openWindow
  @State private var failure: String?

  var body: some View {
    Section {
      LabeledContent {
        Button("Open…", action: openExample)
      } label: {
        RowLabel(Text("Open the example project"), Text("A copy of a ready test for TextEdit, in a folder you choose"))
      }
      .settingAnchor("exampleProject")
      LabeledContent {
        Button("Open Guide") { NSWorkspace.shared.open(VirtualMachineGuide.url()) }
      } label: {
        RowLabel(Text("Run tests in a virtual machine"), Text("A step-by-step guide, so your own screen stays free"))
      }
      .settingAnchor("vmGuide")
    } header: {
      Text("Learn")
    }
    .alert(
      "Couldn’t open the example project",
      isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } }),
      presenting: failure
    ) { _ in
      Button("OK") {}
    } message: { Text($0) }
  }

  private func openExample() {
    do {
      let before = project.rootURL
      try project.openExample()
      if project.rootURL != before { openWindow(id: SceneID.main) }
    } catch {
      AppLog.error("The example project couldn’t be opened: \(error.localizedDescription)", "project")
      failure = error.localizedDescription
    }
  }
}
