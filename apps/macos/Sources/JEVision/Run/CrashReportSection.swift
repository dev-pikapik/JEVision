import SwiftUI

/// In a bug report: a crash report saved on this Mac goes into the description only when the person
/// chooses it. The text lands in the editor, where they can read and change it before sending.
struct CrashReportSection: View {
  @Binding var text: String
  @State private var reports = CrashReports().all()
  @State private var failure: String?

  var body: some View {
    if !reports.isEmpty {
      Section {
        Menu("Add a Crash Report") {
          ForEach(reports) { report in
            Button(report.date.formatted(date: .abbreviated, time: .shortened)) { add(report) }
          }
        }
      } header: {
        Text("Crash Reports")
      } footer: {
        Text("Nothing is added unless you choose a report. It goes into the description above, where you can read and change it before sending.")
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
      .alert(
        "Can’t add the crash report",
        isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } }),
        presenting: failure
      ) { _ in
        Button("OK") {}
      } message: { Text($0) }
    }
  }

  private func add(_ report: CrashReports.Saved) {
    guard let data = try? Data(contentsOf: report.url), let summary = CrashReports.summary(of: data) else {
      failure = String(localized: "The file has no crash in it, or it can’t be read.")
      return
    }
    guard !text.contains(summary) else { return }
    text += (text.isEmpty || text.hasSuffix("\n\n") ? "" : text.hasSuffix("\n") ? "\n" : "\n\n") + summary + "\n"
    AppLog.info("A crash report was added to a bug report", "run")
  }
}
