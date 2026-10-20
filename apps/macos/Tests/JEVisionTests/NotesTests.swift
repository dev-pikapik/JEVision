import Foundation
import JEVisionCore
import Testing

@testable import JEVision

@MainActor
struct NoteDocumentTests {
  private static let chain = """
    {"format": 1, "name": "T", "app": "com.apple.TextEdit", "start": "a", "blocks": {
      "a": {"type": "pressKeys", "keys": "a", "next": "b"},
      "b": {"type": "pressKeys", "keys": "b"}}}
    """

  private func document() throws -> (FlowDocument, URL) {
    let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let url = folder.appending(path: "t.flow.json")
    try Self.chain.write(to: url, atomically: true, encoding: .utf8)
    return (FlowDocument(url: url), url)
  }

  @Test func aNoteAboutABlockStandsBesideItAndIsFoundThroughIt() throws {
    let (document, url) = try document()
    document.author = "Anna"
    document.addNote(about: "b", select: false)
    let note = try #require(document.notes(about: "b").first)
    #expect(note.anchor == .block(id: "b"))
    #expect(note.author == "Anna")
    #expect(document.notes(about: "a").isEmpty)
    let block = try #require(document.layout.blocks.first { $0.id == "b" }?.rect)
    #expect(note.rect.x >= block.x + block.width, "to the right of the block")
    #expect(document.picked.isEmpty, "adding it from the inspector leaves the selection alone")
    #expect(try String(contentsOf: url, encoding: .utf8).contains(#""anchor""#))
  }

  @Test func theTalkUnderANoteIsOneStepEachAndUndoes() throws {
    let (document, _) = try document()
    document.addNote(at: Point(x: 600, y: 40), author: "Anna")
    let id = try #require(document.flow?.canvas.notes.first?.id)
    #expect(document.pickedNote?.id == id)
    #expect(document.reply(to: id, author: "Bo", text: "  ") == nil, "an empty reply isn't added")
    let reply = try #require(document.reply(to: id, author: "Bo", text: "Done, see @Anna"))
    #expect(document.note(id)?.replies.map(\.author) == ["Bo"])
    document.editReply(reply, of: id, text: "Done")
    #expect(document.note(id)?.replies.first?.text == "Done")

    document.resolve(id, by: "Bo")
    #expect(document.note(id)?.resolved?.by == "Bo")
    #expect(document.hasResolvedNotes)
    document.reopen(id)
    #expect(document.note(id)?.resolved == nil)

    let link = Attachment(kind: .link, value: "https://github.com/pikapik/JEVision/issues/7")
    document.attach(link, to: id)
    document.attach(link, to: id)
    #expect(document.note(id)?.attachments == [link], "the same attachment is kept once")
    document.detach(link, from: id)
    #expect(document.note(id)?.attachments.isEmpty == true)

    document.deleteReply(reply, of: id)
    #expect(document.note(id)?.replies.isEmpty == true)
  }

  @Test func settledNotesLeaveTheCanvasWhileTheFilterIsOnUnlessPicked() throws {
    let (document, _) = try document()
    document.addNote(at: Point(x: 600, y: 40), author: "Anna")
    let id = try #require(document.flow?.canvas.notes.first?.id)
    document.resolve(id, by: "Anna")
    let note = try #require(document.note(id))
    #expect(!document.isHidden(note), "shown until the filter is on")
    document.hidesResolvedNotes = true
    document.pick([])
    #expect(document.isHidden(note))
    document.pick([.note(id: id)])
    #expect(!document.isHidden(note), "a picked note stays, so it can be reopened")
  }

  @Test func changingWhatANoteIsAboutRefusesABlockThatIsNotThere() throws {
    let (document, _) = try document()
    document.addNote(at: Point(x: 600, y: 40), author: "Anna")
    let id = try #require(document.flow?.canvas.notes.first?.id)
    document.setAnchor(of: id, to: .block(id: "a"))
    #expect(document.note(id)?.anchor == .block(id: "a"))
    document.setAnchor(of: id, to: .block(id: "nowhere"))
    #expect(document.note(id)?.anchor == .block(id: "a"))
    document.setAnchor(of: id, to: .folder)
    #expect(document.note(id)?.anchor == .folder)
  }

  @Test func deletingABlockAsksWhatToDoWithItsNotes() throws {
    let (document, _) = try document()
    document.addNote(about: "b", select: false)
    document.remove([.block(id: "b")])
    #expect(document.pendingRemoval?.notes.count == 1)
    #expect(document.flow?.blocks.count == 2, "nothing is removed before the answer")

    document.finishRemoval(notes: nil)
    #expect(document.pendingRemoval == nil)
    #expect(document.flow?.blocks.count == 2, "cancel leaves everything")

    document.remove([.block(id: "b")])
    document.finishRemoval(notes: .keep)
    #expect(document.flow?.blocks.count == 1)
    #expect(document.flow?.canvas.notes.first?.anchor == .test, "kept on the canvas, now about the test")

    let (other, _) = try self.document()
    other.addNote(about: "b", select: false)
    other.remove([.block(id: "b")])
    other.finishRemoval(notes: .delete)
    #expect(other.flow?.blocks.count == 1)
    #expect(other.flow?.canvas.notes.isEmpty == true)
  }

  @Test func deletingABlockWithoutNotesOrWithTheNotesToo() throws {
    let (document, _) = try document()
    document.addNote(about: "b", select: false)
    let id = try #require(document.flow?.canvas.notes.first?.id)
    document.remove([.block(id: "b"), .note(id: id)])
    #expect(document.pendingRemoval == nil, "a note deleted on purpose isn't asked about")
    #expect(document.flow?.canvas.notes.isEmpty == true)

    let (plain, _) = try self.document()
    plain.remove([.block(id: "b")])
    #expect(plain.pendingRemoval == nil)
    #expect(plain.flow?.blocks.count == 1)
  }
}

@MainActor
struct NoteTextTests {
  @Test func mentionsBecomeLinksOfTheirOwnScheme() {
    let lines = MarkdownText.lines(of: "Ask @Anna Lee\nand @Bo", names: ["Anna Lee", "Bo"])
    #expect(lines.count == 2)
    #expect(lines[0].contains("\(noteMentionScheme()):"))
  }

  @Test func onlyWebLinksStayLinks() {
    let mention = noteMentionScheme()
    let styled = MarkdownText.styled(
      "[docs](https://example.com) [open](file:///etc/passwd) [run](x-apple.systempreferences:) [@Bo](\(mention):Bo) **bold**")
    let links = styled.runs.compactMap(\.link)
    #expect(links == [URL(string: "https://example.com")], "files and URL schemes are no links, mentions neither")
    #expect(styled.runs.contains { $0.inlinePresentationIntent == .stronglyEmphasized })
  }

  @Test func theLineOfANoteEndsOnTheSidesThatFaceEachOther() {
    let block = CGRect(x: 100, y: 100, width: 100, height: 60)
    let right = NoteLink.ends(note: CGRect(x: 300, y: 100, width: 80, height: 40), block: block)
    #expect(right.0 == CGPoint(x: 300, y: 120) && right.1 == CGPoint(x: 200, y: 130))
    let left = NoteLink.ends(note: CGRect(x: 0, y: 100, width: 80, height: 40), block: block)
    #expect(left.0 == CGPoint(x: 80, y: 120) && left.1 == CGPoint(x: 100, y: 130))
    let above = NoteLink.ends(note: CGRect(x: 110, y: 0, width: 80, height: 40), block: block)
    #expect(above.0 == CGPoint(x: 150, y: 40) && above.1 == CGPoint(x: 150, y: 100))
    let below = NoteLink.ends(note: CGRect(x: 110, y: 200, width: 80, height: 40), block: block)
    #expect(below.0 == CGPoint(x: 150, y: 200) && below.1 == CGPoint(x: 150, y: 160))
  }
}

struct NoteFilesTests {
  @Test func filesLiveNextToTheTestAndNeverTakeAnotherFilesName() throws {
    let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    let test = folder.appending(path: "login/sign-in.flow.json")
    let source = folder.appending(path: "shot.png")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    try Data([1, 2, 3]).write(to: source)

    let first = try NoteFiles.destination(for: source, note: "n1", test: test)
    #expect(first.path.hasSuffix("login/sign-in.assets/notes/n1/shot.png"))
    try NoteFiles.copy(source, to: first)
    #expect(NoteFiles.size(of: first) == 3)
    #expect(NoteFiles.url(of: "shot.png", note: "n1", test: test)?.path == first.path)

    let second = try NoteFiles.destination(for: source, note: "n1", test: test)
    #expect(second != first, "a second file with the name gets another")
    #expect(throws: Error.self) { try NoteFiles.destination(for: source, note: "../n1", test: test) }
    #expect(NoteFiles.url(of: "../shot.png", note: "n1", test: test) == nil)
  }

  @Test func theLargeLimitComesFromSettings() {
    let defaults = UserDefaults.standard
    defer { defaults.removeObject(forKey: NoteFiles.largeKey) }
    defaults.removeObject(forKey: NoteFiles.largeKey)
    #expect(NoteFiles.largeSize == 10_000_000)
    defaults.set(25, forKey: NoteFiles.largeKey)
    #expect(NoteFiles.largeSize == 25_000_000)
    defaults.set(0, forKey: NoteFiles.largeKey)
    #expect(NoteFiles.largeSize == 1_000_000, "never below a megabyte")
  }

  @Test func kindsOfFilesPickTheirSymbol() {
    #expect(NoteFiles.Kind(URL(filePath: "/a/b.png")) == .image)
    #expect(NoteFiles.Kind(URL(filePath: "/a/b.mov")) == .video)
    #expect(NoteFiles.Kind(URL(filePath: "/a/b.pdf")) == .pdf)
    #expect(NoteFiles.Kind(URL(filePath: "/a/b.zip")) == .other)
  }
}

@MainActor
struct NoteAssetsFollowTheTestTests {
  @Test func renamingATestMovesTheFilesOfItsNotes() throws {
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let shot = root.appending(path: "login/sign-in.assets/notes/n1/shot.png")
    try FileManager.default.createDirectory(at: shot.deletingLastPathComponent(), withIntermediateDirectories: true)
    try Data([1]).write(to: shot)
    let flow = #"{"format": 1, "name": "sign-in", "app": "com.apple.TextEdit", "blocks": {}}"#
    try flow.write(to: root.appending(path: "login/sign-in.flow.json"), atomically: true, encoding: .utf8)
    let item = try #require(ProjectScanner.scan(root).first?.children.first)

    try ProjectStore().rename(item, to: "log-in")

    let moved = root.appending(path: "login/log-in.assets/notes/n1/shot.png")
    #expect(FileManager.default.fileExists(atPath: moved.path))
    #expect(!FileManager.default.fileExists(atPath: root.appending(path: "login/sign-in.assets").path))
    #expect(FileManager.default.fileExists(atPath: root.appending(path: "login/log-in.flow.json").path))
  }
}
