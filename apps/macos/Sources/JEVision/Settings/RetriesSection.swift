import JEVisionCore
import SwiftUI

/// Settings › Advanced: the project's default retries and its tests in quarantine, saved in the
/// project's settings file so everyone who runs its tests gets them.
struct RetriesSection: View {
  @Environment(ProjectStore.self) private var project
  @State private var failure: String?

  var body: some View {
    Section {
      if project.rootURL == nil {
        LabeledContent {
          Button("Open Project…", action: project.choose)
        } label: {
          RowLabel(Text("No project is open"), Text("Open the folder with your tests to set its retries and quarantine."))
        }
        .settingAnchor("retries")
      } else if let problem = project.settingsProblem {
        Label(problem, systemImage: "exclamationmark.triangle.fill")
          .foregroundStyle(.red)
          .textSelection(.enabled)
      } else {
        Picker(selection: Binding(get: { project.settings.retries ?? 0 }, set: { set(retries: $0) })) {
          ForEach(UInt32(0)...10, id: \.self) { Text($0, format: .number).tag($0) }
        } label: {
          Text("Retries")
          Text("How many more times to run a failed test whose file doesn’t say. A test that fails, then passes, is marked flaky.")
        }
        .settingAnchor("retries")
        Picker(selection: Binding(get: { quarantineThreshold(settings: project.settings) }, set: { set(threshold: $0) })) {
          ForEach(UInt32(1)...10, id: \.self) { Text($0, format: .number).tag($0) }
        } label: {
          Text("Suggest quarantine after")
          Text("How many times in a week a test may be unreliable before JEVision suggests quarantine: it passed only on a retry, or failed after it passed with nothing changed.")
        }
        if project.settings.quarantine.isEmpty {
          LabeledContent("In quarantine") { Text("No tests") }
        } else {
          ForEach(project.settings.quarantine, id: \.test) { entry in
            LabeledContent {
              Button("Release") { release(entry.test) }
                .help(Text("Release from Quarantine"))
            } label: {
              Label { RowLabel(Text(name(entry.test)), Text(detail(entry))) } icon: { QuarantineMark(entry: entry) }
            }
          }
        }
      }
    } header: {
      Text("Retries and quarantine")
    } footer: {
      Text(
        "\(QuarantineText.explanation) To put a test in quarantine, choose Quarantine… in its menu in the sidebar. The settings are saved in \(projectSettingsFileName()) in the project folder."
      )
      .font(.callout)
      .foregroundStyle(.secondary)
      .fixedSize(horizontal: false, vertical: true)
    }
    .alert(
      "Can’t change the project settings", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } }),
      presenting: failure
    ) { _ in
      Button("OK") {}
    } message: { Text($0) }
  }

  private func name(_ test: String) -> String {
    let file = test.split(separator: "/").last.map(String.init) ?? test
    return file.hasSuffix(ProjectScanner.testSuffix) ? String(file.dropLast(ProjectScanner.testSuffix.count)) : file
  }

  /// Until when and why, or the test file when the entry says neither.
  private func detail(_ entry: Quarantined) -> String {
    entry.until == nil && entry.reason == nil ? entry.test : QuarantineText.describe(entry)
  }

  private func set(threshold: UInt32) {
    do { try project.setQuarantineThreshold(threshold == 3 ? nil : threshold) } catch { failure = ProjectStore.message(error) }
  }

  private func set(retries: UInt32) {
    do { try project.setDefaultRetries(retries == 0 ? nil : retries) } catch { failure = ProjectStore.message(error) }
  }

  private func release(_ test: String) {
    do { try project.releaseFromQuarantine(test) } catch { failure = ProjectStore.message(error) }
  }
}
