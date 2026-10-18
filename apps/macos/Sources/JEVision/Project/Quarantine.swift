import JEVisionCore
import SwiftUI

/// A test about to go to quarantine.
struct QuarantineRequest: Identifiable {
  /// Test file, relative to the project.
  let file: String
  let name: String
  var id: String { file }
}

enum QuarantineText {
  /// "In quarantine" with the reason, for marks and their help.
  static func describe(_ entry: Quarantined) -> String {
    entry.reason.map { String(localized: "In quarantine: \($0)") } ?? String(localized: "In quarantine")
  }

  /// What the quarantine meant for a run of a test that was in it.
  static func describe(_ report: RunReport) -> String {
    let entry = describe(Quarantined(test: report.file, reason: report.quarantineReason))
    return report.verdict == .failed
      ? entry + ". " + String(localized: "The failure doesn’t fail Run All, the command line or the JUnit report.")
      : entry
  }

  static let explanation = String(
    localized:
      "A test in quarantine still runs, but its failures don’t fail Run All, the command line or the JUnit report. The quarantine is saved in the project, so everyone who runs its tests sees it."
  )
}

/// The mark of a test in quarantine, with the reason as its help.
struct QuarantineMark: View {
  let entry: Quarantined

  var body: some View {
    Image(systemName: RunStatus.quarantined.symbol)
      .foregroundStyle(RunStatus.quarantined.color)
      .help(Text(QuarantineText.describe(entry)))
      .accessibilityLabel(Text(QuarantineText.describe(entry)))
  }
}

/// Asks why the test goes to quarantine, then puts it there.
struct QuarantineAlert: ViewModifier {
  @Environment(ProjectStore.self) private var project
  @Binding var request: QuarantineRequest?
  @State private var reason = ""
  @State private var failure: String?

  func body(content: Content) -> some View {
    content
      .alert(
        Text("Quarantine “\(request?.name ?? "")”?"),
        isPresented: Binding(get: { request != nil }, set: { if !$0 { request = nil } }),
        presenting: request
      ) { request in
        TextField("Reason, such as a link to the bug", text: $reason)
        Button("Put in Quarantine") {
          do {
            try project.quarantine(request.file, reason: reason)
          } catch {
            failure = ProjectStore.message(error)
          }
        }
        Button("Cancel", role: .cancel) {}
      } message: { _ in
        Text(QuarantineText.explanation)
      }
      .onChange(of: request?.id) { reason = "" }
      .alert(
        "Can’t change the quarantine", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } }),
        presenting: failure
      ) { _ in
        Button("OK") {}
      } message: { Text($0) }
  }
}

extension View {
  /// Shows the Quarantine alert while `request` is set.
  func quarantineAlert(_ request: Binding<QuarantineRequest?>) -> some View {
    modifier(QuarantineAlert(request: request))
  }
}

/// The Quarantine section of the test's details: whether it's in quarantine, why, and the way out.
struct QuarantineSection: View {
  @Environment(ProjectStore.self) private var project
  let file: String
  let name: String
  @State private var request: QuarantineRequest?
  @State private var failure: String?

  var body: some View {
    Section {
      if let entry = project.quarantine(of: file) {
        LabeledContent {
          Button("Release from Quarantine") {
            do { try project.releaseFromQuarantine(file) } catch { failure = ProjectStore.message(error) }
          }
        } label: {
          Label { Text(QuarantineText.describe(entry)) } icon: { QuarantineMark(entry: entry) }
        }
      } else {
        LabeledContent {
          Button("Quarantine…") { request = QuarantineRequest(file: file, name: name) }
        } label: {
          Text("Not in quarantine")
        }
      }
    } header: {
      Text("Quarantine")
    } footer: {
      Text(QuarantineText.explanation)
    }
    .quarantineAlert($request)
    .alert(
      "Can’t change the quarantine", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } }),
      presenting: failure
    ) { _ in
      Button("OK") {}
    } message: { Text($0) }
  }
}
