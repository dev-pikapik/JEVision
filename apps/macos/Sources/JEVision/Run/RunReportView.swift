import AppKit
import JEVisionCore
import QuickLook
import SwiftUI
import UniformTypeIdentifiers

/// The report of one saved run: why it ended as it did, and every step with its screenshots.
struct RunReportView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(RunCenter.self) private var runs
  /// The run's folder.
  let dir: String
  @State private var run: SavedRun?
  @State private var loadError: String?
  /// Tests the steps belong to, as they are now: for the blocks' descriptions.
  @State private var flows: [String: Flow] = [:]
  @State private var attempt = 0
  @State private var expanded: Set<UInt32> = []
  @State private var preview: URL?
  @State private var failure: String?

  var body: some View {
    Group {
      if let run {
        report(run)
      } else if let loadError {
        ContentUnavailableView {
          Label("Can’t open this report", systemImage: "exclamationmark.triangle")
        } description: {
          Text(loadError)
        }
      } else {
        ProgressView()
      }
    }
    .task(id: dir) { load() }
    .alert(
      "Something went wrong", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } }),
      presenting: failure
    ) { _ in
      Button("OK") {}
    } message: { Text($0) }
  }

  private func load() {
    do {
      let loaded = try loadRun(dir: dir)
      run = loaded
      attempt = max(0, loaded.report.attempts.count - 1)
      // A failure or a new reference is what the report is opened for: show it at once.
      let new = loaded.report.attempts.last?.steps.filter { $0.status == .baselineCreated }.map(\.seq) ?? []
      expanded = Set(new + (RunHistory.failedStep(loaded.report).map { [$0.seq] } ?? []))
      if let root = project.rootURL {
        let files = Set(loaded.report.attempts.flatMap { $0.steps.map(\.test) } + [loaded.report.file])
        flows = files.reduce(into: [:]) { flows, file in
          let text = try? String(contentsOf: root.appending(path: file), encoding: .utf8)
          flows[file] = text.flatMap { try? loadFlow(json: $0) }
        }
      }
    } catch {
      loadError = RunHistory.reason(error)
    }
  }

  private func report(_ run: SavedRun) -> some View {
    let report = run.report
    let steps = report.attempts.indices.contains(attempt) ? report.attempts[attempt].steps : []
    let images = steps.flatMap { StepImages(step: $0, in: run.url).all }
    return ScrollView {
      VStack(alignment: .leading, spacing: 16) {
        ReportHeader(
          run: run, hasTest: project.hasTest, isRunning: runs.isRunning,
          openFailedBlock: { step in project.showBlock(step.block, in: step.test) },
          runAgain: { runAgain(report) },
          export: { export(run, as: $0) })
        VStack(alignment: .leading, spacing: 8) {
          HStack {
            Text("Steps")
              .font(.headline)
            Spacer()
            if report.attempts.count > 1 {
              Picker("Try", selection: $attempt) {
                ForEach(report.attempts.indices, id: \.self) { index in
                  Text("Try \(index + 1)").tag(index)
                }
              }
              .pickerStyle(.segmented)
              .labelsHidden()
              .fixedSize()
            }
          }
          if steps.isEmpty {
            Text("The test stopped before its first step.")
              .foregroundStyle(.secondary)
              .padding(.vertical, 8)
          } else {
            VStack(spacing: 0) {
              ForEach(Array(steps.enumerated()), id: \.element.seq) { index, step in
                if index > 0 { Divider().padding(.leading, 44) }
                StepRow(
                  step: step, run: run, flow: flows[step.test], isExpanded: expanded.contains(step.seq),
                  toggle: {
                    if expanded.remove(step.seq) == nil { expanded.insert(step.seq) }
                  },
                  preview: { preview = $0 },
                  failure: $failure)
              }
            }
            .padding(.horizontal, 6)
            .background(.background, in: .rect(cornerRadius: 14))
            .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(.separator))
          }
        }
      }
      .padding(20)
      .frame(maxWidth: 960)
      .frame(maxWidth: .infinity)
    }
    .quickLookPreview($preview, in: images)
    .navigationTitle(report.test)
    .navigationSubtitle(Text("Run Report"))
  }

  private func runAgain(_ report: RunReport) {
    guard let root = project.rootURL else { return }
    project.selection = report.file
    guard let document = project.document else { return }
    runs.run(document, in: root)
  }

  private func export(_ run: SavedRun, as format: ReportHeader.Export) {
    let panel = NSSavePanel()
    let day = run.startedAt.formatted(Date.ISO8601FormatStyle(timeZone: .current).year().month().day())
    let base = "\(run.report.test) \(day)"
    switch format {
    case .html:
      panel.nameFieldStringValue = base + ".html"
      panel.allowedContentTypes = [.html]
    case .junit:
      panel.nameFieldStringValue = base + ".xml"
      panel.allowedContentTypes = [.xml]
    }
    panel.canCreateDirectories = true
    panel.isExtensionHidden = false
    guard panel.runModal() == .OK, let url = panel.url else { return }
    do {
      switch format {
      case .html: try exportHtml(dir: run.dir, to: url.path)
      case .junit: try exportJunit(dirs: [run.dir], to: url.path)
      }
    } catch {
      failure = String(localized: "Can’t save the report: \(RunHistory.reason(error))")
    }
  }
}

/// The reason in one sentence, what the retries say, and what to do next.
private struct ReportHeader: View {
  enum Export { case html, junit }

  let run: SavedRun
  /// Whether a test file, relative to the project, is still there.
  let hasTest: (String) -> Bool
  let isRunning: Bool
  let openFailedBlock: (Step) -> Void
  let runAgain: () -> Void
  let export: (Export) -> Void

  var body: some View {
    let report = run.report
    let status = run.status
    let failed = RunHistory.failedStep(report)
    // A passed run's retries are the whole story; a failed run's say whether to trust the failure.
    let verdict = RunHistory.retryVerdict(report)
    let headline = report.verdict == .passed ? verdict ?? RunText.outcome(report) : RunText.outcome(report)
    HStack(alignment: .top, spacing: 14) {
      Image(systemName: status.symbol)
        .font(.system(size: 34))
        .foregroundStyle(status.color)
        .accessibilityHidden(true)
      VStack(alignment: .leading, spacing: 6) {
        Text(headline)
          .font(.title2.weight(.semibold))
          .textSelection(.enabled)
          .fixedSize(horizontal: false, vertical: true)
          .accessibilityAddTraits(.isHeader)
          .accessibilityLabel(Text("\(status.title): \(headline)"))
        Text(meta(report))
          .foregroundStyle(.secondary)
        if report.verdict == .failed, let verdict {
          Label(verdict, systemImage: "arrow.clockwise")
            .foregroundStyle(.secondary)
        }
        HStack(spacing: 8) {
          if let failed {
            Button("Open Failed Block", systemImage: "scope") { openFailedBlock(failed) }
              .buttonStyle(.glassProminent)
              .disabled(!hasTest(failed.test))
              .help(Text("Show the block in the editor"))
          }
          Button("Run Again", systemImage: "arrow.clockwise", action: runAgain)
            .buttonStyle(.glass)
            .disabled(!hasTest(report.file) || isRunning)
          Menu {
            Button("HTML Report…") { export(.html) }
            Button("JUnit XML…") { export(.junit) }
          } label: {
            Label("Export", systemImage: "square.and.arrow.up")
          }
          .menuStyle(.button)
          .buttonStyle(.glass)
          .fixedSize()
          .help(Text("Save the report as one HTML file with its screenshots, or as JUnit XML for CI"))
          Button("Show in Finder", systemImage: "folder") {
            NSWorkspace.shared.activateFileViewerSelecting([run.url])
          }
          .buttonStyle(.glass)
        }
        .padding(.top, 6)
      }
      Spacer(minLength: 0)
    }
    .padding(18)
    .background(.background, in: .rect(cornerRadius: 18))
    .overlay(RoundedRectangle(cornerRadius: 18).strokeBorder(.separator))
  }

  private func meta(_ report: RunReport) -> String {
    let when = run.startedAt.formatted(date: .abbreviated, time: .shortened)
    let parts = [report.test, when, RunHistory.duration(report.durationMs)]
    return parts.filter { !$0.isEmpty }.joined(separator: " · ")
  }
}

/// Screenshots a step left, as files.
struct StepImages {
  var before: URL?
  var after: URL?
  var expected: URL?
  var diff: URL?

  init(step: Step, in run: URL) {
    let file = { (path: String?) in path.map { run.appending(path: $0) } }
    before = file(step.before)
    after = file(step.after)
    expected = file(step.expected)
    diff = file(step.diff)
  }

  var all: [URL] { [before, expected, after, diff].compactMap(\.self) }
  /// The step compared the screen with a reference.
  var isComparison: Bool { expected != nil && after != nil }
}

/// One step of the report; a click shows its screenshots.
private struct StepRow: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.undoManager) private var undoManager
  let step: Step
  let run: SavedRun
  let flow: Flow?
  let isExpanded: Bool
  let toggle: () -> Void
  let preview: (URL) -> Void
  @Binding var failure: String?
  @State private var confirmsAccept = false

  private var images: StepImages { StepImages(step: step, in: run.url) }
  private var canExpand: Bool { !images.all.isEmpty || step.failure != nil }

  private var title: String { BlockInfo.title(step.blockType) }

  private var summary: String {
    let block = flow?.blocks.first { $0.id == step.block }
    var text = block.map { BlockSummary.text($0.kind, appName: AppCatalog.shared.name(of:)) } ?? step.block
    if let branch = step.branch {
      text += " · " + (branch == "then" ? String(localized: "answer: Yes") : String(localized: "answer: No"))
    }
    if step.test != run.report.file {
      text += " · " + String(localized: "in \(ProjectScanner.testName((step.test as NSString).lastPathComponent))")
    }
    return text
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 0) {
      Group {
        if canExpand {
          Button(action: toggle) { header }
            .buttonStyle(.plain)
        } else {
          header
        }
      }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text("Step \(step.seq), \(title): \(summary)"))
        .accessibilityValue(Text(accessibilityValue))
        .accessibilityHint(canExpand ? Text(isExpanded ? "Hides the screenshots" : "Shows the screenshots") : Text(verbatim: ""))
      if isExpanded {
        detail
          .padding(.leading, 44)
          .padding(.trailing, 8)
          .padding(.bottom, 14)
      }
    }
    .confirmationDialog(
      "Make this screenshot the reference?", isPresented: $confirmsAccept, titleVisibility: .visible
    ) {
      Button("Accept as New Reference", action: accept)
    } message: {
      Text("Next runs compare with it. The current image \(step.reference ?? "") moves to the Trash, and Edit › Undo puts it back.")
    }
  }

  private var accessibilityValue: String {
    let status = StepStatusText(step.status)
    let parts = [status.title, RunHistory.duration(step.durationMs)] + (step.failure.map { [RunText.failure($0)] } ?? [])
    return parts.joined(separator: ", ")
  }

  private var header: some View {
    let status = StepStatusText(step.status)
    return HStack(spacing: 10) {
      Text(step.seq, format: .number)
        .font(.caption.monospacedDigit())
        .foregroundStyle(.tertiary)
        .frame(width: 20, alignment: .trailing)
      IconTile(symbol: BlockInfo.symbol(step.blockType), color: BlockInfo.color(ofType: step.blockType), size: 26)
      VStack(alignment: .leading, spacing: 2) {
        Text(title)
          .fontWeight(.medium)
        Text(summary)
          .font(.subheadline)
          .foregroundStyle(.secondary)
          .lineLimit(1)
          .truncationMode(.middle)
        if let failure = step.failure {
          Text(RunText.failure(failure))
            .font(.subheadline)
            .foregroundStyle(.red)
            .lineLimit(isExpanded ? nil : 2)
        }
      }
      Spacer(minLength: 8)
      Text(RunHistory.duration(step.durationMs))
        .font(.callout.monospacedDigit())
        .foregroundStyle(.secondary)
      Image(systemName: status.symbol)
        .foregroundStyle(status.color)
        .help(Text(status.title))
      Image(systemName: "chevron.right")
        .font(.caption.weight(.semibold))
        .foregroundStyle(.tertiary)
        .rotationEffect(.degrees(isExpanded ? 90 : 0))
        .opacity(canExpand ? 1 : 0)
    }
    .padding(.vertical, 8)
    .padding(.horizontal, 4)
    .contentShape(.rect)
  }

  @ViewBuilder private var detail: some View {
    let images = images
    VStack(alignment: .leading, spacing: 10) {
      if step.status == .baselineCreated, let after = images.after {
        Label(
          "There was no reference yet, so this screenshot became it: \(step.reference ?? "")",
          systemImage: "photo.badge.checkmark")
          .foregroundStyle(.secondary)
        ShotImage(url: after, label: String(localized: "New reference image"), preview: preview)
      } else if images.isComparison {
        Comparison(step: step, images: images, preview: preview)
        acceptButton(images)
      } else {
        HStack(alignment: .top, spacing: 12) {
          if let before = images.before {
            ShotImage(url: before, label: String(localized: "Before"), preview: preview)
          }
          if let after = images.after {
            ShotImage(url: after, label: String(localized: "After"), preview: preview)
          }
        }
      }
    }
  }

  @ViewBuilder private func acceptButton(_ images: StepImages) -> some View {
    if step.blockType == "check", step.status == .failed, let after = images.after, let reference = step.reference,
      let root = project.rootURL, let file = Baseline.file(reference, in: root)
    {
      // Read so the button comes back after Undo.
      let _ = project.referenceChanges
      if Baseline.isReference(after, file) {
        Label("This screenshot is the reference now.", systemImage: "checkmark.circle")
          .foregroundStyle(.secondary)
      } else {
        Button("Accept as New Reference…", systemImage: "checkmark.seal") { confirmsAccept = true }
          .buttonStyle(.glass)
          .help(Text("Replace the reference image with this screenshot"))
      }
    }
  }

  private func accept() {
    guard let after = images.after, let reference = step.reference else { return }
    do {
      try project.acceptReference(after, as: reference, undoManager: undoManager)
    } catch {
      failure = String(localized: "Can’t replace the reference image: \(error.localizedDescription)")
    }
  }
}

/// A step's result in words and as a symbol.
private struct StepStatusText {
  let title: String
  let symbol: String
  let color: Color

  init(_ status: StepStatus) {
    switch status {
    case .passed: (title, symbol, color) = (String(localized: "Passed"), "checkmark.circle.fill", .green)
    case .failed: (title, symbol, color) = (String(localized: "Failed"), "xmark.square.fill", .red)
    case .baselineCreated:
      (title, symbol, color) = (String(localized: "New reference saved"), "plus.circle.fill", .blue)
    case .cancelled: (title, symbol, color) = (String(localized: "Stopped"), "stop.circle.fill", .secondary)
    }
  }
}
