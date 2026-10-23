import SwiftUI

/// In a bug report: a crash report saved on this Mac and the app log go into the description only
/// when the person chooses each of them. The text lands in the editor, where they can read and
/// change it before sending.
struct DiagnosticsSection: View {
  @Binding var text: String
  @State private var reports = CrashReports().all()
  @State private var hasLog = FileManager.default.fileExists(atPath: RotatingLog(folder: AppLog.folder).url(0).path)
  @State private var failure: String?

  static let logHeading = "### App log"

  var body: some View {
    if !reports.isEmpty || hasLog {
      Section {
        if !reports.isEmpty {
          LabeledContent {
            Menu("Add a Crash Report") {
              ForEach(reports) { report in
                Button(report.date.formatted(date: .abbreviated, time: .shortened)) { add(report) }
              }
            }
            .fixedSize()
          } label: {
            Text("Crash report")
            Text("How JEVision crashed, saved after you turned it on in Settings")
          }
        }
        if hasLog {
          LabeledContent {
            Button("Add the Log", action: addLog)
              .disabled(text.contains(Self.logHeading))
          } label: {
            Text("App log")
            Text("The newest lines of what JEVision did. Secrets are never in it.")
          }
        }
      } header: {
        Text("From This Mac")
      } footer: {
        Text("Nothing is added unless you choose it. It goes into the description above, where you can read and change it before sending.")
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
      .alert(
        "Can’t add it to the bug report",
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
    append(summary)
    AppLog.info("A crash report was added to a bug report", "run")
  }

  private func addLog() {
    guard let log = Self.log(AppLog.tail()) else {
      failure = String(localized: "The log is empty, or it can’t be read.")
      return
    }
    append(log)
    AppLog.info("The app log was added to a bug report", "run")
  }

  private func append(_ block: String) {
    text += (text.isEmpty || text.hasSuffix("\n\n") ? "" : text.hasSuffix("\n") ? "\n" : "\n\n") + block + "\n"
  }

  /// The log's lines as a part of the description, or `nil` when there are none.
  static func log(_ lines: [String]) -> String? {
    guard !lines.isEmpty else { return nil }
    return ([logHeading, "The newest lines of JEVision’s log on this Mac, the oldest first:", "```"] + lines + ["```"])
      .joined(separator: "\n")
  }
}
