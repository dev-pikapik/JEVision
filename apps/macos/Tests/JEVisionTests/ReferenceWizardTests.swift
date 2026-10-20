import Foundation
import JEVisionCore
import Testing

@testable import JEVision

/// A project with one picture, refs/home.png, shared by three tests (one a custom block), next to
/// a test that doesn't use it, and a run's screenshot to make the new reference.
private struct SharedPicture {
  let root = FileManager.default.temporaryDirectory.appending(path: "jevision-reference-\(UUID().uuidString)")
  var path: String { root.path(percentEncoded: false) }
  var picture: URL { root.appending(path: "refs/home.png") }
  var screenshot: URL { root.appending(path: "shot.png") }
  var trash: URL { root.appending(path: "Trash") }
  static let tests = ["login.flow.json", "smoke/inbox.flow.json", "blocks/open-home.block.json", "other.flow.json"]

  init() throws {
    try write(
      "login.flow.json",
      """
      "a":{"type":"check","reference":{"image":"refs/home.png","compare":"similar","threshold":0.9,
        "ignore":[{"x":0.1,"y":0.1,"width":0.1,"height":0.1}]},"next":"b"},
      "b":{"type":"check","reference":{"image":"refs/login.png"}}
      """)
    try write(
      "smoke/inbox.flow.json",
      """
      "a":{"type":"waitFor","until":{"appears":{"image":"../refs/home.png"}},"timeout":"5s","next":"b"},
      "b":{"type":"click","target":{"image":"../refs/home.png"}}
      """)
    try write(
      "blocks/open-home.block.json",
      """
      "a":{"type":"if","reference":{"image":"../refs/other.png"},"then":"b","elseIf":[
        {"reference":{"image":"../refs/home.png","area":{"x":0,"y":0,"width":1,"height":0.5}},"then":"b"}]},
      "b":{"type":"pause","duration":"1s"}
      """)
    try write("other.flow.json", #""a":{"type":"check","reference":{"image":"refs/other.png"}}"#)
    try FileManager.default.createDirectory(at: picture.deletingLastPathComponent(), withIntermediateDirectories: true)
    try FileManager.default.createDirectory(at: trash, withIntermediateDirectories: true)
    try Data("old".utf8).write(to: picture)
    try Data("new".utf8).write(to: screenshot)
  }

  private func write(_ file: String, _ blocks: String) throws {
    let url = root.appending(path: file)
    try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
    let text = """
      {"format":1,"name":"\(file)","app":"com.example.App","start":"a","blocks":{\(blocks)}}
      """
    try Data(text.utf8).write(to: url)
  }

  func text(_ file: String) -> String? { try? String(contentsOf: root.appending(path: file), encoding: .utf8) }

  func moveToTrash(_ url: URL) throws -> URL {
    let target = trash.appending(path: url.lastPathComponent)
    try FileManager.default.moveItem(at: url, to: target)
    return target
  }

  var uses: [ReferenceUse] { referenceUses(project: path, reference: "refs/home.png") }

  /// The failed check of login.flow.json block a.
  var step: Step {
    Step(
      seq: 1, test: "login.flow.json", block: "a", blockType: "check", status: .failed, durationMs: 10, before: nil,
      after: "shot.png", expected: nil, diff: nil, reference: "refs/home.png", similarity: 0.5, vision: nil,
      branch: nil, failure: nil)
  }

  func remove() { try? FileManager.default.removeItem(at: root) }
}

@MainActor
struct ReferenceWizardTests {
  private func wizard(_ project: SharedPicture, uses: [ReferenceUse]? = nil) throws -> ReferenceWizard {
    try #require(ReferenceWizard(step: project.step, run: project.root, uses: uses ?? project.uses))
  }

  private static func tighter(_ threshold: Double) -> (inout Reference) -> Void {
    { $0.compare = .similar(threshold: threshold) }
  }

  @Test func listsEveryBlockThatUsesThePictureWithTheFailedOneFirst() throws {
    let project = try SharedPicture()
    defer { project.remove() }
    // The files are listed by path, so the custom block comes before the failed test.
    #expect(project.uses.first?.test == "blocks/open-home.block.json")
    let wizard = try wizard(project)
    #expect(wizard.entries.map(\.use.test) == ["login.flow.json", "blocks/open-home.block.json", "smoke/inbox.flow.json", "smoke/inbox.flow.json"])
    #expect(wizard.entries.map(\.use.field) == ["reference", "elseIf.0.reference", "until.appears", "target"])
    #expect(wizard.entries[3].settings == nil, "a click on the picture has no settings")
  }

  @Test func goesThroughEveryBlockAndSummarizes() throws {
    let project = try SharedPicture()
    defer { project.remove() }
    let wizard = try wizard(project)
    #expect(!wizard.isSingle)
    #expect(wizard.pages == [.compare, .affected, .block(0), .block(1), .block(2), .block(3), .finish])
    #expect(wizard.position == 1 && !wizard.canGoBack)

    wizard.next()
    wizard.next()
    #expect(wizard.page == .block(0) && wizard.position == 3 && wizard.canGoBack)
    wizard.edit(0, Self.tighter(0.8))
    wizard.decide(0, .confirmed)
    #expect(wizard.page == .block(1))
    wizard.decide(1, .skipped)
    wizard.decide(2, .confirmed)
    wizard.decide(3, .confirmed)
    #expect(wizard.page == .finish)
    wizard.back()
    #expect(wizard.page == .block(3))
    wizard.next()

    #expect(wizard.summary == ReferenceWizard.Outcome(blocks: 3, tests: 2, skipped: 1))
    #expect(wizard.edits.map(\.test) == ["login.flow.json"], "only confirmed blocks whose settings changed are written")
    #expect(wizard.edits.first?.settings.compare == .similar(threshold: 0.8))
  }

  @Test func skippingDropsTheChangesAndResetPutsTheBlocksOwnBack() throws {
    let project = try SharedPicture()
    defer { project.remove() }
    let wizard = try wizard(project)
    wizard.edit(0, Self.tighter(0.7))
    #expect(wizard.entries[0].isEdited)
    wizard.reset(0)
    #expect(!wizard.entries[0].isEdited)
    wizard.edit(0, Self.tighter(0.7))
    wizard.decide(0, .skipped)
    #expect(!wizard.entries[0].isEdited)
    #expect(wizard.edits.isEmpty)
    // A block without settings has nothing to change.
    wizard.edit(3, Self.tighter(0.7))
    #expect(wizard.entries[3].settings == nil)
  }

  @Test func collapsesToOneStepForOneBlockOrNone() throws {
    let project = try SharedPicture()
    defer { project.remove() }
    for uses in [Array(project.uses.prefix(1)), []] {
      let wizard = try wizard(project, uses: uses)
      #expect(wizard.isSingle)
      #expect(wizard.pages == [.compare])
      #expect(wizard.entries.allSatisfy { $0.decision == .confirmed }, "accepting is the one decision")
      #expect(!wizard.canGoBack)
      wizard.next()
      #expect(wizard.page == .compare)
    }
    let one = try wizard(project, uses: Array(project.uses.prefix(1)))
    one.edit(0, Self.tighter(0.7))
    #expect(one.edits.count == 1)
  }

  @Test func needsAScreenshotAndAReference() throws {
    let project = try SharedPicture()
    defer { project.remove() }
    var step = project.step
    step.after = nil
    #expect(ReferenceWizard(step: step, run: project.root, uses: []) == nil)
    step = project.step
    step.reference = nil
    #expect(ReferenceWizard(step: step, run: project.root, uses: []) == nil)
  }

  @Test func afterTheLastStepNothingMoves() throws {
    let project = try SharedPicture()
    defer { project.remove() }
    let wizard = try wizard(project)
    wizard.finished()
    #expect(wizard.outcome == wizard.summary)
    wizard.next()
    wizard.decide(0, .skipped)
    #expect(wizard.page == .compare && wizard.entries[0].decision == .pending)
  }
}

struct ReferenceAcceptanceTests {
  private func edits(_ project: SharedPicture, _ threshold: Double) -> [ReferenceEdit] {
    project.uses.compactMap { use in
      guard var settings = use.settings else { return nil }
      settings.compare = .similar(threshold: threshold)
      settings.ignore = [Rect(x: 0.5, y: 0.5, width: 0.25, height: 0.25)]
      return ReferenceEdit(test: use.test, block: use.block, field: use.field, settings: settings)
    }
  }

  @Test func replacesThePictureAndTheSettingsOfEveryTestAndTakesThemBack() throws {
    let project = try SharedPicture()
    defer { project.remove() }
    let before = SharedPicture.tests.map { project.text($0) }

    var accepted = try ReferenceAcceptance.accept(
      project.screenshot, as: project.picture, in: project.root, edits: edits(project, 0.8), trash: project.moveToTrash)
    #expect(project.text("refs/home.png") == "new")
    #expect(accepted.old.flatMap { try? String(contentsOf: $0, encoding: .utf8) } == "old")
    #expect(accepted.change?.files.count == 3)
    #expect(project.text("login.flow.json")?.contains("\"threshold\" : 0.8") == true || project.text("login.flow.json")?.contains("\"threshold\": 0.8") == true)
    #expect(project.text("other.flow.json") == before[3], "a test that doesn't use the picture is left alone")

    try ReferenceAcceptance.undo(accepted, in: project.root)
    #expect(project.text("refs/home.png") == "old")
    #expect(SharedPicture.tests.map { project.text($0) } == before)

    accepted = try ReferenceAcceptance.redo(accepted, in: project.root, trash: project.moveToTrash)
    #expect(project.text("refs/home.png") == "new")
    #expect(project.text("login.flow.json") != before[0])
  }

  @Test func changesNothingWhenThePictureCannotBeReplaced() throws {
    let project = try SharedPicture()
    defer { project.remove() }
    let before = SharedPicture.tests.map { project.text($0) }
    let gone = project.root.appending(path: "no-such-shot.png")
    #expect(throws: (any Error).self) {
      try ReferenceAcceptance.accept(
        gone, as: project.picture, in: project.root, edits: edits(project, 0.8), trash: project.moveToTrash)
    }
    #expect(project.text("refs/home.png") == "old")
    #expect(SharedPicture.tests.map { project.text($0) } == before)
  }

  @Test func changesNothingWhenASettingDoesNotFit() throws {
    let project = try SharedPicture()
    defer { project.remove() }
    let before = SharedPicture.tests.map { project.text($0) }
    #expect(throws: ReferenceError.self) {
      try ReferenceAcceptance.accept(
        project.screenshot, as: project.picture, in: project.root, edits: edits(project, 1.5), trash: project.moveToTrash)
    }
    #expect(project.text("refs/home.png") == "old")
    #expect(SharedPicture.tests.map { project.text($0) } == before)
  }

  @Test func doesNotTakeBackOverATestChangedSince() throws {
    let project = try SharedPicture()
    defer { project.remove() }
    let accepted = try ReferenceAcceptance.accept(
      project.screenshot, as: project.picture, in: project.root, edits: edits(project, 0.8), trash: project.moveToTrash)
    let inbox = project.root.appending(path: "smoke/inbox.flow.json")
    let edited = (project.text("smoke/inbox.flow.json") ?? "") + " "
    try Data(edited.utf8).write(to: inbox)

    #expect(throws: ReferenceError.Changed(test: "smoke/inbox.flow.json")) {
      try ReferenceAcceptance.undo(accepted, in: project.root)
    }
    #expect(project.text("refs/home.png") == "new", "the picture stays with the settings")
    #expect(project.text("smoke/inbox.flow.json") == edited)
  }

  @Test func justThePictureWhenNoSettingsChanged() throws {
    let project = try SharedPicture()
    defer { project.remove() }
    let before = SharedPicture.tests.map { project.text($0) }
    let accepted = try ReferenceAcceptance.accept(
      project.screenshot, as: project.picture, in: project.root, edits: [], trash: project.moveToTrash)
    #expect(accepted.change == nil)
    #expect(project.text("refs/home.png") == "new")
    try ReferenceAcceptance.undo(accepted, in: project.root)
    #expect(project.text("refs/home.png") == "old")
    #expect(SharedPicture.tests.map { project.text($0) } == before)
  }

  @Test @MainActor func saysWhatWentWrongInWords() {
    let message = ProjectStore.message(ReferenceError.Changed(test: "smoke/inbox.flow.json"))
    #expect(message.contains("smoke/inbox.flow.json"))
  }
}
