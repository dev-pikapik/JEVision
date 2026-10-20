import JEVisionCore
import SwiftUI

/// One file a pull couldn't combine on its own, as the user settles it. A test that opens on
/// both sides is settled block by block; any other file is kept as one side has it.
struct ConflictFile: Identifiable {
  /// The three versions of a test, and what both sides changed differently.
  struct Test {
    let base: Flow?
    let mine: Flow
    let theirs: Flow
    let conflicts: [MergeConflict]
  }

  let path: String
  let versions: ConflictVersions
  let test: Test?
  /// One per conflict of the test; one for the whole file otherwise.
  var choices: [MergeChoice]

  var id: String { path }

  init(path: String, versions: ConflictVersions) {
    self.path = path
    self.versions = versions
    let flow = { (data: Data?) in data.flatMap { String(data: $0, encoding: .utf8) }.flatMap { try? loadFlow(json: $0) } }
    if GitText.isTest(path), let mine = flow(versions.mine), let theirs = flow(versions.theirs),
      versions.base == nil || flow(versions.base) != nil
    {
      let base = flow(versions.base)
      let conflicts = mergeConflicts(base: base, mine: mine, theirs: theirs)
      test = Test(base: base, mine: mine, theirs: theirs, conflicts: conflicts)
      choices = Array(repeating: .mine, count: conflicts.count)
    } else {
      test = nil
      choices = [.mine]
    }
  }

  /// The test as the choices combine it.
  var merged: Result<Flow, FlowError>? {
    guard let test else { return nil }
    return Result { try mergeFlows(base: test.base, mine: test.mine, theirs: test.theirs, choices: choices) }
      .mapError { $0 as? FlowError ?? .Invalid(issues: []) }
  }

  /// What keeps the combined test from running, in words; empty when it can be saved.
  var problems: [String] {
    switch merged {
    case nil: []
    case .success(let flow)?: validateFlow(flow: flow).filter { $0.severity == .error }.map(IssueText.message)
    case .failure(.Invalid(let issues))?:
      issues.isEmpty ? [String(localized: "The combined test can’t be read.")] : issues.map(IssueText.message)
    }
  }

  /// The file as settled: the combined test in canonical form, or the chosen side's file;
  /// no contents when that side removed it.
  var settled: SettledFile? {
    guard let merged else {
      return SettledFile(path: path, contents: choices.first == .theirs ? versions.theirs : versions.mine)
    }
    guard case .success(let flow) = merged, problems.isEmpty else { return nil }
    return SettledFile(path: path, contents: Data(saveFlow(flow: flow).utf8))
  }

  /// Settles every conflict of the file the same way; `both` only where that makes sense.
  mutating func chooseAll(_ choice: MergeChoice) {
    guard let test else {
      choices = [choice == .theirs ? .theirs : .mine]
      return
    }
    choices = test.conflicts.map { choice == .both && !$0.canKeepBoth ? .mine : choice }
  }
}

/// Settles the files a pull couldn't combine, then brings in the server's changes with one
/// merge commit. Each test shows both versions side by side with the conflicting blocks
/// outlined; what only one side changed is already combined.
struct ConflictsView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  let paths: [String]

  @State private var files: [ConflictFile]?
  @State private var selection: String?
  @State private var loadFailure: String?

  private var git: ProjectGit { project.git }
  private var isFinishing: Bool { git.activity == .pulling }
  private var settled: [SettledFile]? {
    guard let files else { return nil }
    let settled = files.compactMap(\.settled)
    return settled.count == files.count ? settled : nil
  }

  var body: some View {
    VStack(spacing: 0) {
      VStack(spacing: 2) {
        Text("Combine Changes")
          .font(.headline)
          .accessibilityAddTraits(.isHeader)
        Text("You and the server changed the same files. Choose what to keep — nothing changes until you combine them.")
          .font(.subheadline)
          .foregroundStyle(.secondary)
          .multilineTextAlignment(.center)
      }
      .padding(.vertical, 10)
      .padding(.horizontal)
      Divider()
      content
      Divider()
      footer
    }
    .frame(minWidth: 900, idealWidth: 1100, minHeight: 560, idealHeight: 720)
    .task(id: paths) { await load() }
    .alert(
      "Couldn’t combine the changes",
      isPresented: Binding(get: { git.failure?.activity == .pulling }, set: { if !$0 { git.failure = nil } }),
      presenting: git.failure
    ) { _ in
      Button("OK") {}
    } message: { Text($0.message) }
  }

  @ViewBuilder
  private var content: some View {
    if let files {
      if files.count > 1 {
        HSplitView {
          List(files, selection: $selection) { file in
            FileRow(file: file)
              .tag(file.path)
          }
          .frame(minWidth: 220, idealWidth: 260, maxWidth: 340)
          detail
            .frame(minWidth: 560, maxWidth: .infinity, maxHeight: .infinity)
        }
      } else {
        detail
          .frame(maxWidth: .infinity, maxHeight: .infinity)
      }
    } else if let loadFailure {
      ContentUnavailableView {
        Label("Can’t read the changed files", systemImage: "exclamationmark.triangle")
      } description: {
        Text(loadFailure)
      } actions: {
        Button("Try Again") { Task { await load() } }
      }
      .frame(maxHeight: .infinity)
    } else {
      ProgressView()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityLabel(Text("Reading both versions"))
    }
  }

  @ViewBuilder
  private var detail: some View {
    if let index = files?.firstIndex(where: { $0.path == selection }) ?? files?.indices.first,
      let binding = Binding($files)?[index]
    {
      ConflictFileView(file: binding)
        .id(binding.wrappedValue.path)
    }
  }

  private var footer: some View {
    HStack {
      Menu("Use for All") {
        Button("Mine") { chooseAll(.mine) }
        Button("Theirs") { chooseAll(.theirs) }
        Button("Both Where Possible") { chooseAll(.both) }
      }
      .fixedSize()
      .disabled(files == nil || isFinishing)
      .help(Text("Settle every conflict in every file the same way."))
      Spacer()
      if isFinishing {
        ProgressView()
          .controlSize(.small)
          .accessibilityLabel(Text("Combining"))
      }
      Button("Cancel", role: .cancel) {
        git.conflicts = nil
        dismiss()
      }
      .keyboardShortcut(.cancelAction)
      Button("Combine and Commit") { finish() }
        .keyboardShortcut(.defaultAction)
        .disabled(settled == nil || isFinishing)
        .help(Text("Bring in the server’s changes with your choices and commit the result."))
    }
    .padding()
  }

  private func chooseAll(_ choice: MergeChoice) {
    files = files?.map { file in
      var file = file
      file.chooseAll(choice)
      return file
    }
  }

  private func finish() {
    guard let settled else { return }
    git.finishPull(settled) {
      git.conflicts = nil
      dismiss()
    }
  }

  private func load() async {
    guard let root = project.rootURL else { return }
    let path = root.path(percentEncoded: false)
    let paths = paths
    let found = await Task.detached(priority: .userInitiated) {
      Result {
        try paths.map { ConflictFile(path: $0, versions: try gitConflictVersions(project: path, path: $0)) }
      }
    }.value
    guard !Task.isCancelled else { return }
    switch found {
    case .success(let found):
      files = found
      if !found.contains(where: { $0.path == selection }) { selection = found.first?.path }
      loadFailure = nil
    case .failure(let error):
      loadFailure = GitText.failure(error)
    }
  }
}

/// A file in the list: its name and how many conflicts are left in it.
private struct FileRow: View {
  let file: ConflictFile

  var body: some View {
    HStack(spacing: 8) {
      Image(systemName: file.settled == nil ? "exclamationmark.triangle.fill" : "checkmark.circle.fill")
        .foregroundStyle(file.settled == nil ? .orange : .green)
        .accessibilityHidden(true)
      VStack(alignment: .leading, spacing: 1) {
        Text(verbatim: GitText.fileName(file.path))
          .lineLimit(1)
        Text(ConflictText.count(file))
          .font(.caption)
          .foregroundStyle(.secondary)
      }
    }
    .accessibilityElement(children: .combine)
  }
}

/// One file: both versions of a test with its conflicts, or a choice of side for another file.
private struct ConflictFileView: View {
  enum Shown: Hashable {
    case sides, result
  }

  @Binding var file: ConflictFile
  @State private var shown = Shown.sides

  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: 16) {
        if let test = file.test {
          testView(test)
        } else {
          wholeFile
        }
      }
      .padding()
    }
  }

  @ViewBuilder
  private func testView(_ test: ConflictFile.Test) -> some View {
    Picker("Show", selection: $shown) {
      Text("Both Versions").tag(Shown.sides)
      Text("Result").tag(Shown.result)
    }
    .pickerStyle(.segmented)
    .labelsHidden()
    .fixedSize()
    .frame(maxWidth: .infinity)
    switch shown {
    case .sides:
      HStack(alignment: .top, spacing: 16) {
        side(Text("Mine"), detail: Text("On this Mac"), flow: test.mine, base: test.base, test: test)
        Divider()
        side(Text("Theirs"), detail: Text("On the server"), flow: test.theirs, base: test.base, test: test)
      }
    case .result:
      if case .success(let flow)? = file.merged {
        let diff = diffFlows(before: test.mine, after: flow)
        Text("Your version with the server’s changes as chosen. Highlighted: what changes in yours.")
          .font(.subheadline)
          .foregroundStyle(.secondary)
        DiffCanvas(
          flow: flow, blocks: FlowDiffView.marks(diff.blocks, on: .after),
          notes: FlowDiffView.marks(diff.notes, on: .after), sections: FlowDiffView.marks(diff.sections, on: .after))
      }
    }
    Divider()
    if test.conflicts.isEmpty {
      Label("Nothing to choose: the changes on both sides fit together and are combined.", systemImage: "checkmark.circle.fill")
        .foregroundStyle(.green)
    } else {
      Text(ConflictText.count(file))
        .font(.headline)
      ForEach(Array(test.conflicts.enumerated()), id: \.offset) { index, conflict in
        ConflictRow(conflict: conflict, test: test, choice: $file.choices[index])
      }
    }
    let problems = file.problems
    if !problems.isEmpty {
      VStack(alignment: .leading, spacing: 4) {
        Label("The combined test has problems. Choose differently to fix them:", systemImage: "xmark.octagon.fill")
          .foregroundStyle(.red)
        ForEach(problems, id: \.self) { Text(verbatim: "• \($0)") }
      }
      .accessibilityElement(children: .combine)
    }
  }

  /// One version of the test with what it changed highlighted and its conflicts outlined.
  private func side(_ title: Text, detail: Text, flow: Flow, base: Flow?, test: ConflictFile.Test) -> some View {
    let diff = diffFlows(before: base, after: flow)
    let conflicted = { (part: ConflictPart) in Set(test.conflicts.filter { $0.part == part }.map(\.id)) }
    let marks = { (changes: [BlockChange], part: ConflictPart) in
      FlowDiffView.marks(changes, on: .after).merging(conflicted(part).map { ($0, DiffMark.conflict) }) { $1 }
    }
    return VStack(alignment: .leading, spacing: 8) {
      VStack(alignment: .leading, spacing: 1) {
        title.font(.headline)
        detail.font(.subheadline).foregroundStyle(.secondary)
      }
      .accessibilityElement(children: .combine)
      .accessibilityAddTraits(.isHeader)
      DiffCanvas(
        flow: flow, blocks: marks(diff.blocks, .block), notes: marks(diff.notes, .note),
        sections: marks(diff.sections, .frame))
    }
    .frame(maxWidth: .infinity, alignment: .top)
  }

  @ViewBuilder
  private var wholeFile: some View {
    let versions = file.versions
    Text(verbatim: GitText.fileName(file.path))
      .font(.title3.bold())
    Text(ConflictText.wholeFile(versions, isTest: GitText.isTest(file.path)))
      .foregroundStyle(.secondary)
    if GitText.isPicture(file.path) {
      HStack(alignment: .top, spacing: 16) {
        picture(Text("Mine"), versions.mine)
        picture(Text("Theirs"), versions.theirs)
      }
    }
    Picker("Keep", selection: $file.choices[0]) {
      Text(versions.mine == nil ? "Mine: Removed" : "Mine").tag(MergeChoice.mine)
      Text(versions.theirs == nil ? "Theirs: Removed" : "Theirs").tag(MergeChoice.theirs)
    }
    .pickerStyle(.segmented)
    .fixedSize()
    .accessibilityLabel(Text("Which version of “\(GitText.fileName(file.path))” to keep"))
  }

  private func picture(_ title: Text, _ data: Data?) -> some View {
    VStack(alignment: .leading, spacing: 6) {
      title.font(.headline)
      if let data, let image = NSImage(data: data) {
        Image(nsImage: image)
          .resizable()
          .scaledToFit()
          .frame(maxHeight: 280)
          .accessibilityLabel(title)
      } else {
        Text("Removed")
          .foregroundStyle(.secondary)
      }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }
}

/// One conflict: what each side has and the choice between them.
private struct ConflictRow: View {
  let conflict: MergeConflict
  let test: ConflictFile.Test
  @Binding var choice: MergeChoice

  var body: some View {
    let title = ConflictText.title(conflict, test)
    HStack(alignment: .firstTextBaseline, spacing: 12) {
      Image(systemName: DiffMark.conflict.symbol)
        .foregroundStyle(DiffMark.conflict.color)
        .accessibilityHidden(true)
      VStack(alignment: .leading, spacing: 3) {
        Text(verbatim: title)
          .font(.headline)
        Text(ConflictText.kind(conflict.kind))
          .font(.subheadline)
          .foregroundStyle(.secondary)
        Text("Mine: \(ConflictText.value(conflict, in: test.mine))")
          .textSelection(.enabled)
        Text("Theirs: \(ConflictText.value(conflict, in: test.theirs))")
          .textSelection(.enabled)
      }
      .frame(maxWidth: .infinity, alignment: .leading)
      Picker("Keep", selection: $choice) {
        Text("Mine").tag(MergeChoice.mine)
        Text("Theirs").tag(MergeChoice.theirs)
        if conflict.canKeepBoth { Text("Both").tag(MergeChoice.both) }
      }
      .pickerStyle(.segmented)
      .labelsHidden()
      .fixedSize()
      .accessibilityLabel(Text("Keep for \(title)"))
    }
    .padding(10)
    .background(.orange.opacity(0.06), in: .rect(cornerRadius: 10))
  }
}

/// Conflicts in words.
@MainActor
enum ConflictText {
  /// “2 conflicts” or “Nothing to choose”.
  static func count(_ file: ConflictFile) -> String {
    guard let test = file.test else { return String(localized: "Choose which version to keep") }
    return test.conflicts.isEmpty
      ? String(localized: "Nothing to choose") : String(localized: "\(test.conflicts.count) conflicts")
  }

  /// What the conflict is about, such as “Wait: Time limit” or “Test: Name”.
  static func title(_ conflict: MergeConflict, _ test: ConflictFile.Test) -> String {
    switch conflict.part {
    case .test:
      let field = conflict.field.map { $0 == "start" ? String(localized: "First step") : FlowDiffView.fieldName($0) }
      return String(localized: "Test: \(field ?? "")")
    case .block:
      let kind = block(conflict.id, in: test.mine)?.kind ?? block(conflict.id, in: test.theirs)?.kind
      let name = kind.map { BlockInfo.title($0.typeName) } ?? conflict.id
      guard let field = conflict.field else { return name }
      return "\(name): \(conflict.kind == .link ? linkName(field, kind) : FlowDiffView.fieldName(field))"
    case .note:
      guard let field = conflict.field else { return String(localized: "Note") }
      return String(localized: "Note: \(FlowDiffView.fieldName(field))")
    case .frame: return String(localized: "Frame")
    }
  }

  static func kind(_ kind: ConflictKind) -> String {
    switch kind {
    case .changed: String(localized: "Changed on both sides, differently.")
    case .link: String(localized: "Each side leads it to a different step.")
    case .removedHere: String(localized: "You removed it; on the server it was changed.")
    case .removedThere: String(localized: "Removed on the server; you changed it.")
    case .addedBoth: String(localized: "Both sides added it, each differently.")
    }
  }

  /// What one side has for the conflict, in words.
  static func value(_ conflict: MergeConflict, in flow: Flow) -> String {
    switch conflict.part {
    case .test: return testValue(conflict.field ?? "", flow)
    case .block:
      guard let found = block(conflict.id, in: flow) else { return String(localized: "Removed") }
      if conflict.kind == .link, let field = conflict.field {
        let to = layoutFlow(flow: flow).edges.first { $0.from == conflict.id && $0.edge == field }?.to
        return to.flatMap { block($0, in: flow) }.map(describe) ?? String(localized: "Leads nowhere")
      }
      return describe(found)
    case .note:
      return flow.canvas.notes.first { $0.id == conflict.id }.map { noteValue($0, field: conflict.field, in: flow) }
        ?? String(localized: "Removed")
    case .frame:
      return flow.canvas.sections.first { $0.id == conflict.id }
        .map { $0.title.isEmpty ? String(localized: "Untitled Frame") : $0.title } ?? String(localized: "Removed")
    }
  }

  /// What a note has for the part of it two sides changed differently.
  private static func noteValue(_ note: Note, field: String?, in flow: Flow) -> String {
    switch field {
    case "anchor":
      switch note.anchor {
      case .test: return String(localized: "About the test")
      case .folder: return String(localized: "About the folder")
      case .block(let id):
        let title = block(id, in: flow).map { BlockInfo.title($0.kind.typeName) } ?? id
        return String(localized: "About the block \(title)")
      }
    case "resolved":
      guard let resolved = note.resolved else { return String(localized: "Open") }
      return resolved.by.isEmpty ? String(localized: "Resolved") : String(localized: "Resolved by \(resolved.by)")
    case "replies":
      return note.replies.isEmpty
        ? String(localized: "No replies") : note.replies.map { BlockSummary.quoted($0.text, limit: 40) }.joined(separator: " · ")
    case "attachments":
      return note.attachments.isEmpty
        ? String(localized: "Nothing attached") : note.attachments.map(\.value).joined(separator: ", ")
    default:
      return BlockSummary.quoted(note.text, limit: 60)
    }
  }

  /// What a file that can't be combined block by block went through.
  static func wholeFile(_ versions: ConflictVersions, isTest: Bool) -> String {
    switch (versions.mine, versions.theirs) {
    case (nil, _): String(localized: "You removed this file; on the server it was changed.")
    case (_, nil): String(localized: "This file was removed on the server; you changed it.")
    default:
      isTest
        ? String(localized: "One version of this test can’t be opened, so it can’t be combined block by block.")
        : String(localized: "Both sides changed this file.")
    }
  }

  private static func block(_ id: String, in flow: Flow) -> Block? { flow.blocks.first { $0.id == id } }

  private static func describe(_ block: Block) -> String {
    let summary = BlockSummary.text(block.kind, appName: { AppCatalog.shared.name(of: $0) })
    let title = BlockInfo.title(block.kind.typeName)
    return summary.isEmpty ? title : "\(title) — \(summary)"
  }

  private static func linkName(_ edge: String, _ kind: BlockKind?) -> String {
    if let name = kind?.branchName(edge) { return name }
    return edge == "body" ? String(localized: "Steps inside") : String(localized: "Next step")
  }

  private static func testValue(_ field: String, _ flow: Flow) -> String {
    let none = String(localized: "Not set")
    switch field {
    case "name": return flow.name
    case "description": return flow.description ?? none
    case "app": return AppCatalog.shared.name(of: flow.app)
    case "testType": return flow.testType.map(TestFields.title) ?? none
    case "priority": return flow.priority.map(TestFields.title) ?? none
    case "status": return flow.status.map(TestFields.title) ?? none
    case "tags": return flow.tags.isEmpty ? none : flow.tags.formatted(.list(type: .and))
    case "retries": return flow.retries.map { String($0) } ?? String(localized: "Project default")
    case "start": return block(flow.start, in: flow).map(describe) ?? none
    default: return none
    }
  }
}
