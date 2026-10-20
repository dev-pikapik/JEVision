import Foundation
import JEVisionCore
import Observation

/// Making a picture the new reference picture when other blocks compare with the same picture:
/// look at the old and the new one, see which blocks use it, and check each block's settings
/// (areas, masks, similarity) against the new picture. Nothing is written until the end, so
/// cancelling leaves everything as it was.
@MainActor
@Observable
final class ReferenceWizard: Identifiable {
  /// Where the new picture comes from.
  enum Source {
    /// The screenshot of a failed check, in the folder of its run.
    case run(Step, folder: URL)
    /// A picture chosen or captured in the reference editor of `block` in `test`; `old` is the
    /// picture it replaces.
    case editor(test: String, block: String, old: URL?)
  }

  enum Page: Hashable {
    /// The old, the new and the differences. With one block or none, its settings follow on this page.
    case compare
    /// Every block that uses the picture.
    case affected
    /// The settings of the block at this place in `entries`, on the new picture.
    case block(Int)
    /// What is about to change.
    case finish
  }

  enum Decision: Equatable {
    case pending, confirmed, skipped
  }

  /// A block that uses the picture, with the settings being reviewed.
  struct Entry: Identifiable {
    let use: ReferenceUse
    /// The block's own settings until they are changed; `nil` for a block with none, like a
    /// click that looks for the picture on the screen.
    var settings: Reference?
    var decision = Decision.pending

    var id: String { "\(use.test)\n\(use.block)\n\(use.field)" }
    var isEdited: Bool { settings != use.settings }
  }

  /// What the wizard did.
  struct Outcome: Equatable {
    /// Blocks confirmed, with or without changed settings.
    var blocks: Int
    /// The tests and custom blocks those blocks are in.
    var tests: Int
    /// Blocks left as they are.
    var skipped: Int
  }

  let id = UUID()
  let source: Source
  /// The picture to replace, relative to the project.
  let reference: String
  /// The new picture.
  let screenshot: URL
  /// Keeps the new picture's file for as long as an Undo can bring it back.
  let keeping: TemporaryPicture?
  private(set) var entries: [Entry]
  private(set) var page = Page.compare
  /// Set once the picture is replaced.
  private(set) var outcome: Outcome?
  /// Why the picture couldn't be replaced.
  var failure: String?

  /// `step` is the failed check the screenshot is from; `uses` are the blocks of the project that
  /// use its reference picture. The failed block comes first.
  init?(step: Step, run: URL, uses: [ReferenceUse]) {
    guard let after = step.after, let reference = step.reference else { return nil }
    source = .run(step, folder: run)
    self.reference = reference
    screenshot = run.appending(path: after)
    keeping = nil
    entries = Self.entries(uses, first: (step.test, step.block))
  }

  /// `picture` is to replace `reference` (relative to `project`), the picture of `block` in the test
  /// `file` that is open in the reference editor. `nil` when no other block uses `reference`: then
  /// there is nothing to review. The block being edited comes first, and the guide starts with
  /// the list of blocks.
  init?(replacing reference: String, with picture: TemporaryPicture, in project: URL, file: URL, block: String) {
    let uses = referenceUses(project: project.path(percentEncoded: false), reference: reference)
    guard uses.count > 1 else { return nil }
    let test = RelativePath.from(project, to: file)
    source = .editor(test: test, block: block, old: Baseline.file(reference, in: project))
    self.reference = reference
    screenshot = picture.url
    keeping = picture
    entries = Self.entries(uses, first: (test, block))
    page = .affected
  }

  private static func entries(_ uses: [ReferenceUse], first: (test: String, block: String)) -> [Entry] {
    let isFirst = { (use: ReferenceUse) in use.test == first.test && use.block == first.block }
    let all = uses.filter(isFirst) + uses.filter { !isFirst($0) }
    return all.map { Entry(use: $0, settings: $0.settings, decision: all.count < 2 ? .confirmed : .pending) }
  }


  /// The picture being replaced, when the guide comes from the reference editor.
  var old: URL? {
    if case .editor(_, _, let old) = source { old } else { nil }
  }

  /// The test and block the guide was started from: the failed check, or the block being edited.
  var origin: (test: String, block: String) {
    switch source {
    case .run(let step, _): (step.test, step.block)
    case .editor(let test, let block, _): (test, block)
    }
  }

  /// One block or none: there is nothing to go through, and accepting is the one decision.
  var isSingle: Bool { entries.count < 2 }

  var pages: [Page] {
    let blocks = entries.indices.map(Page.block) + [.finish]
    return switch source {
    case .run: isSingle ? [.compare] : [.compare, .affected] + blocks
    case .editor: [.affected] + blocks
    }
  }

  /// Where `page` comes in `pages`, counted from 1.
  var position: Int { (pages.firstIndex(of: page) ?? 0) + 1 }

  var canGoBack: Bool { outcome == nil && position > 1 }

  func back() {
    guard canGoBack else { return }
    page = pages[position - 2]
  }

  func next() {
    guard outcome == nil, position < pages.count else { return }
    page = pages[position]
  }

  /// Confirms or skips the block at `index` and goes on. Skipping drops the changes made to it.
  func decide(_ index: Int, _ decision: Decision) {
    guard entries.indices.contains(index), outcome == nil else { return }
    entries[index].decision = decision
    if decision == .skipped { entries[index].settings = entries[index].use.settings }
    next()
  }

  func edit(_ index: Int, _ change: (inout Reference) -> Void) {
    guard entries.indices.contains(index), outcome == nil, var settings = entries[index].settings else { return }
    change(&settings)
    entries[index].settings = settings
  }

  /// Puts the block's own settings back.
  func reset(_ index: Int) {
    guard entries.indices.contains(index), outcome == nil else { return }
    entries[index].settings = entries[index].use.settings
  }

  /// The settings to write: those of confirmed blocks that were changed.
  var edits: [ReferenceEdit] {
    entries.compactMap { entry in
      guard entry.decision == .confirmed, entry.isEdited, let settings = entry.settings else { return nil }
      return ReferenceEdit(test: entry.use.test, block: entry.use.block, field: entry.use.field, settings: settings)
    }
  }

  /// How many blocks and tests the wizard has confirmed and skipped so far.
  var summary: Outcome {
    let confirmed = entries.filter { $0.decision == .confirmed }
    return Outcome(
      blocks: confirmed.count, tests: Set(confirmed.map(\.use.test)).count,
      skipped: entries.filter { $0.decision == .skipped }.count)
  }

  /// Records that the picture is replaced.
  func finished() {
    outcome = summary
    failure = nil
  }
}
