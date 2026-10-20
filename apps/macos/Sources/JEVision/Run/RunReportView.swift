import AVKit
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
  @State private var player: AVPlayer?
  @Namespace private var stepsNamespace

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
      player = RunVideoView.file(of: loaded).map(AVPlayer.init(url:))
      attempt = Self.decidingIndex(loaded.report)
      // A failure or a new reference is what the report is opened for: show it at once.
      let new = decidingAttempt(report: loaded.report)?.steps.filter { $0.status == .baselineCreated }.map(\.seq) ?? []
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
    let row = report.attempts.indices.contains(attempt) ? report.attempts[attempt].row : nil
    let tries = report.attempts.indices.filter { report.attempts[$0].row == row }
    let images = steps.flatMap { StepImages(step: $0, in: run.url).all }
    return ScrollView {
      VStack(alignment: .leading, spacing: 16) {
        ReportHeader(
          run: run, hasTest: project.hasTest, isRunning: runs.isRunning,
          openFailedBlock: { step in project.showBlock(step.block, in: step.test, from: run.id) },
          runAgain: { runAgain(report) },
          reportBug: { project.bugReportRun = run.dir },
          export: { export(run, as: $0) })
        if report.verdict == .failed {
          FailureAnalysisCard(
            run: run, runAgain: { runAgain(report) },
            openFailedBlock: { step in project.showBlock(step.block, in: step.test, from: run.id) }
          )
          .id(run.id)
        }
        if let player {
          RunVideoView(player: player, steps: steps)
        }
        if !report.rows.isEmpty {
          RowResults(report: report, shown: row?.number) { number in
            attempt = report.attempts.lastIndex { $0.row?.number == number } ?? attempt
            expanded = Set(report.attempts[attempt].steps.filter { $0.status == .failed && $0.failure != nil }.suffix(1).map(\.seq))
          }
        }
        VStack(alignment: .leading, spacing: 8) {
          HStack {
            if let row {
              Text("Steps of \(RunChoices.rowTitle(number: Int(row.number), label: row.label))")
                .font(.headline)
                .accessibilityAddTraits(.isHeader)
            } else {
              Text("Steps")
                .font(.headline)
                .accessibilityAddTraits(.isHeader)
            }
            Spacer()
            if tries.count > 1 {
              Picker("Try", selection: $attempt) {
                ForEach(tries, id: \.self) { index in
                  Text("Try \(Int(report.attempts[index].number))").tag(index)
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
            let depths = Self.depths(steps.map(\.test), in: report.file)
            VStack(spacing: 0) {
              ForEach(Array(steps.enumerated()), id: \.element.seq) { index, step in
                if index > 0 { Divider().padding(.leading, 44) }
                StepRow(
                  step: step, run: run, flow: flows[step.test], isExpanded: expanded.contains(step.seq),
                  toggle: {
                    if expanded.remove(step.seq) == nil {
                      expanded.insert(step.seq)
                      if let player { RunVideoView.seek(player, to: step) }
                    }
                  },
                  preview: { preview = $0 },
                  failure: $failure)
                .padding(.leading, Double(depths[index]) * 24)
                .accessibilityRotorEntry(id: step.seq, in: stepsNamespace)
              }
            }
            .accessibilityRotor(Text("Failed Steps")) {
              ForEach(steps.filter { $0.status == .failed }, id: \.seq) { step in
                AccessibilityRotorEntry(
                  Text("Step \(Int(step.seq)), \(BlockInfo.title(step.blockType))"), id: step.seq, in: stepsNamespace)
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

  /// Index of the try that decided the verdict, the one a report is opened for.
  /// How deep each step, given by the test it belongs to, runs inside other tests and custom
  /// blocks: 0 for the steps of `file` itself, 1 for those of a test it runs, and so on.
  static func depths(_ tests: [String], in file: String) -> [Int] {
    var running = [file]
    return tests.map { test in
      if let level = running.lastIndex(of: test) {
        running.removeSubrange((level + 1)...)
      } else {
        running.append(test)
      }
      return running.count - 1
    }
  }

  static func decidingIndex(_ report: RunReport) -> Int {
    let deciding = decidingAttempt(report: report)
    return report.attempts.lastIndex { $0.number == deciding?.number && $0.row == deciding?.row }
      ?? max(0, report.attempts.count - 1)
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
      case .junit: try exportJunit(dirs: [run.dir], to: url.path, includeQuarantinedFailures: false)
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
  let reportBug: () -> Void
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
        .accessibilityLabel(Text(status.title))
      VStack(alignment: .leading, spacing: 6) {
        // Selectable text is read by its value, so a label of its own would be read as well.
        Text(RunText.heading(headline))
          .font(.title2.weight(.semibold))
          .textSelection(.enabled)
          .fixedSize(horizontal: false, vertical: true)
          .accessibilityAddTraits(.isHeader)
        Text(meta(report))
          .foregroundStyle(.secondary)
        if report.verdict == .failed, let verdict {
          Label(verdict, systemImage: "arrow.clockwise")
            .foregroundStyle(.secondary)
        }
        if report.quarantined {
          Label {
            Text(QuarantineText.describe(report))
          } icon: {
            Image(systemName: RunStatus.quarantined.symbol).foregroundStyle(RunStatus.quarantined.color)
          }
          .foregroundStyle(.secondary)
          .textSelection(.enabled)
        }
        if report.addedAsDependency {
          Label("Added to the run because other tests in it depend on this one", systemImage: "arrow.turn.down.right")
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
          if report.verdict == .failed {
            Button("Report Bug…", systemImage: "ladybug", action: reportBug)
              .buttonStyle(.glass)
              .help(Text("File an issue on GitHub with the steps, the cause and the screenshots"))
          }
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
    let environment = report.environment.map { String(localized: "Environment: \($0)") }
    let parts = [report.test, when, environment ?? ""] + report.machine + [RunHistory.duration(report.durationMs)]
    return parts.filter { !$0.isEmpty }.joined(separator: " · ")
  }
}

extension RunReport {
  /// Where the run ran, such as “Virtual machine” and “Standard display (1×)”; empty for a
  /// report that doesn't say.
  var machine: [String] {
    let computer = virtualMachine.map { $0 ? String(localized: "Virtual machine") : String(localized: "Physical Mac") }
    let display = displayScale.map { scale in
      let times = scale.formatted()
      return scale >= 2 ? String(localized: "Retina display (\(times)×)") : String(localized: "Standard display (\(times)×)")
    }
    return [computer, display].compactMap(\.self)
  }
}

/// Screenshots a step left, as files.
struct StepImages {
  var before: URL?
  var after: URL?
  var expected: URL?
  var diff: URL?
  /// The picture a vision model was shown, private areas painted over.
  var sent: URL?

  init(step: Step, in run: URL) {
    let file = { (path: String?) in path.map { run.appending(path: $0) } }
    before = file(step.before)
    after = file(step.after)
    expected = file(step.expected)
    diff = file(step.diff)
    sent = file(step.sent)
  }

  var all: [URL] { [before, expected, after, diff, sent].compactMap(\.self) }
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
  private var canExpand: Bool { !images.all.isEmpty || step.failure != nil || step.vision != nil }

  private var title: String { BlockInfo.title(step.blockType) }

  private var summary: String {
    let block = flow?.blocks.first { $0.id == step.block }
    var text = block.map { BlockSummary.text($0.kind, appName: AppCatalog.shared.name(of:)) } ?? step.block
    if let branch = RunText.branch(step) {
      text += " · " + branch
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
    var parts = [status.title, RunHistory.duration(step.durationMs)]
    if !step.outputs.isEmpty { parts.append(String(localized: "saved \(outputs)")) }
    parts += step.failure.map { [RunText.failure($0)] } ?? []
    return parts.joined(separator: ", ")
  }

  /// Values the block saved, as `name = value`; secrets are already masked by the core.
  private var outputs: String {
    step.outputs.map { "\($0.name) = \($0.value)" }.joined(separator: " · ")
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
        if !step.outputs.isEmpty {
          Label {
            Text(verbatim: outputs)
              .font(.subheadline.monospaced())
              .lineLimit(isExpanded ? nil : 1)
              .truncationMode(.tail)
          } icon: {
            Image(systemName: "arrow.down.doc")
          }
          .foregroundStyle(.secondary)
          .help(Text("Values saved for later blocks"))
        }
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
      // A failed check by meaning already gives the model's reason.
      if let answer = step.vision, step.failure?.code != "run.check.meaningMismatch" {
        Label {
          Text(verbatim: "\(answer.provider) · \(answer.model): \(answer.reason)")
            .textSelection(.enabled)
            .fixedSize(horizontal: false, vertical: true)
        } icon: {
          Image(systemName: "sparkles")
        }
        .foregroundStyle(.secondary)
      }
      if let answer = step.vision {
        AIUsageNote(answer: answer)
      }
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
      if let sent = images.sent {
        ShotImage(
          url: sent,
          found: step.found,
          label: step.sentMasked
            ? String(localized: "Sent to the AI model, masked before sending")
            : String(localized: "Sent to the AI model"),
          preview: preview)
        if !step.found.isEmpty {
          Text("The red outline shows where JEV found the element.")
            .font(.callout)
            .foregroundStyle(.secondary)
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

/// How each row of a data-driven run ended; picking a row shows its steps.
private struct RowResults: View {
  let report: RunReport
  /// Number of the row whose steps are shown.
  let shown: UInt32?
  let show: (UInt32) -> Void

  var body: some View {
    let passed = report.rows.count { $0.verdict == .passed }
    VStack(alignment: .leading, spacing: 8) {
      HStack(alignment: .firstTextBaseline) {
        Text("Data Rows")
          .font(.headline)
        Spacer()
        Text("Passed: \(passed) of \(report.rows.count)")
          .foregroundStyle(.secondary)
      }
      VStack(spacing: 0) {
        ForEach(Array(report.rows.enumerated()), id: \.element.row.number) { index, result in
          if index > 0 { Divider().padding(.leading, 44) }
          Button { show(result.row.number) } label: {
            row(result)
          }
          .buttonStyle(.plain)
          .background(result.row.number == shown ? Color.accentColor.opacity(0.12) : .clear, in: .rect(cornerRadius: 8))
          .accessibilityAddTraits(result.row.number == shown ? .isSelected : [])
          .accessibilityHint(Text("Shows the steps of this row"))
        }
      }
      .padding(.horizontal, 6)
      .background(.background, in: .rect(cornerRadius: 14))
      .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(.separator))
    }
  }

  private func row(_ result: RowResult) -> some View {
    HStack(alignment: .firstTextBaseline, spacing: 10) {
      Image(systemName: Self.symbol(result.verdict))
        .foregroundStyle(Self.color(result.verdict))
        .frame(width: 24)
        .accessibilityLabel(Text(Self.verdict(result.verdict)))
      VStack(alignment: .leading, spacing: 2) {
        Text(RunChoices.rowTitle(number: Int(result.row.number), label: result.row.label))
          .lineLimit(1)
          .truncationMode(.tail)
        if let failure = result.failure, result.verdict == .failed {
          Text(RunText.failure(failure))
            .font(.callout)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        } else if result.flaky {
          Text("Passed on try \(Int(result.attempts)), so it may be flaky: it failed before that.")
            .font(.callout)
            .foregroundStyle(.secondary)
        }
      }
      Spacer(minLength: 0)
      if result.attempts > 1 {
        Text("Tries: \(Int(result.attempts))")
          .font(.callout)
          .foregroundStyle(.secondary)
          .monospacedDigit()
      }
    }
    .padding(.horizontal, 8)
    .padding(.vertical, 8)
    .contentShape(.rect)
  }

  private static func symbol(_ verdict: Verdict) -> String {
    switch verdict {
    case .passed: "checkmark.circle.fill"
    case .failed: "xmark.square.fill"
    case .cancelled: "stop.circle.fill"
    case .blocked: "minus.circle.fill"
    }
  }

  private static func color(_ verdict: Verdict) -> Color {
    switch verdict {
    case .passed: .green
    case .failed: .red
    case .cancelled: .secondary
    case .blocked: .yellow
    }
  }

  private static func verdict(_ verdict: Verdict) -> String {
    switch verdict {
    case .passed: String(localized: "Passed")
    case .failed: String(localized: "Failed")
    case .cancelled: String(localized: "Stopped")
    case .blocked: String(localized: "Blocked")
    }
  }
}

/// What an AI step cost, and whether it came near a limit or needed the fallback provider.
private struct AIUsageNote: View {
  let answer: VisionAnswer

  var body: some View {
    if answer.inputTokens > 0 || answer.outputTokens > 0 {
      Text(usage)
        .font(.callout)
        .foregroundStyle(.secondary)
    }
    if let reason = answer.fallbackReason {
      Label {
        Text("Answered by \(answer.provider) instead: \(VisionText.failure(reason))")
          .fixedSize(horizontal: false, vertical: true)
      } icon: {
        Image(systemName: "arrow.triangle.branch")
      }
      .foregroundStyle(.secondary)
    }
    if let warning = answer.warning {
      Label {
        Text(VisionText.failure(warning))
          .fixedSize(horizontal: false, vertical: true)
      } icon: {
        Image(systemName: "exclamationmark.triangle.fill")
          .foregroundStyle(.orange)
      }
    }
  }

  private var usage: String {
    let tokens = String(localized: "\(answer.inputTokens) tokens sent, \(answer.outputTokens) received")
    guard let cost = answer.cost else { return String(localized: "\(tokens) · no price for this model") }
    return "\(tokens) · \(AIMoney.text(cost))"
  }
}
