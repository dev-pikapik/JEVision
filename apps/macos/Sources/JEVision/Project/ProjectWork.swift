import AppKit

/// A test that runs or is being recorded now. Closing its project or opening another would cut
/// it short, so the person is asked first.
struct ProjectWork {
  enum Kind {
    case run, recording
  }

  let kind: Kind
  /// The name of the test.
  let test: String
  /// Stops it cleanly: a run ends as stopped and is kept with the others, a recording adds the
  /// steps recorded so far to the test.
  let stop: @MainActor () -> Void

  /// What runs or is being recorded now; `nil` when nothing is.
  @MainActor
  static func current(runs: RunCenter, records: RecordCenter) -> ProjectWork? {
    if let recording = records.recording {
      return ProjectWork(kind: .recording, test: name(of: recording.document), stop: records.done)
    }
    if let running = runs.running {
      return ProjectWork(kind: .run, test: name(of: running.document), stop: runs.stop)
    }
    return nil
  }

  @MainActor
  private static func name(of document: FlowDocument) -> String {
    let name = document.flow?.name.trimmingCharacters(in: .whitespaces) ?? ""
    return name.isEmpty ? ProjectScanner.testName(document.url.lastPathComponent) : name
  }

  /// Asks whether to stop `work` and go on; `true` when the person agrees.
  @MainActor
  static func ask(_ work: ProjectWork) -> Bool {
    let alert = NSAlert()
    switch work.kind {
    case .run:
      alert.messageText = String(localized: "A test is running")
      alert.informativeText = String(
        localized: "“\(work.test)” is running now. To go on, JEVision stops it, and the run is kept as stopped.")
    case .recording:
      alert.messageText = String(localized: "A test is being recorded")
      alert.informativeText = String(
        localized: "“\(work.test)” is being recorded now. To go on, JEVision stops recording and adds the steps recorded so far to the test.")
    }
    alert.addButton(withTitle: String(localized: "Stop and Continue"))
    alert.addButton(withTitle: String(localized: "Cancel"))
    NSApp.activate()
    return alert.runModal() == .alertFirstButtonReturn
  }
}
