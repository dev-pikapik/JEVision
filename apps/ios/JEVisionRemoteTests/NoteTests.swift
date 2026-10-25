import CoreGraphics
import Foundation
import JEVisionMobile
import Testing

@testable import JEVisionRemote

struct NoteMappingTests {
  private func summary(
    anchor: String = "block", block: String? = "open", time: String = "2026-10-19T10:00:00.000Z", replies: [JEVisionMobile.NoteReplySummary] = [],
    resolved: JEVisionMobile.NoteResolvedSummary? = nil, attachments: [JEVisionMobile.NoteAttachmentSummary] = []
  ) -> JEVisionMobile.NoteSummary {
    JEVisionMobile.NoteSummary(
      id: "n", text: "**Heads up**", author: "Anna", time: time, anchor: anchor, block: block,
      area: JEVisionMobile.Area(x: 320, y: 16, width: 224, height: 144), replyCount: UInt32(replies.count), replies: replies,
      resolved: resolved, attachments: attachments)
  }

  @Test func aNoteCrossesFromTheCoreWithItsDiscussion() throws {
    let note = TestNote(
      summary(
        replies: [JEVisionMobile.NoteReplySummary(id: "r", author: "Boris", time: "2026-10-19T11:00:00Z", text: "Seen it too")],
        resolved: JEVisionMobile.NoteResolvedSummary(by: "Boris", time: "2026-10-19T12:00:00Z"),
        attachments: [
          JEVisionMobile.NoteAttachmentSummary(kind: "file", value: "shot.png"),
          JEVisionMobile.NoteAttachmentSummary(kind: "link", value: "https://github.com/o/r/issues/1"),
        ]))
    #expect((note.id, note.text, note.author) == ("n", "**Heads up**", "Anna"))
    #expect(note.time == (try remoteDate("2026-10-19T10:00:00.000Z")))
    #expect(note.anchor == .block("open"))
    #expect(note.area == CGRect(x: 320, y: 16, width: 224, height: 144))
    #expect(note.replyCount == 1)
    #expect(note.replies == [TestNote.Reply(id: "r", author: "Boris", time: try remoteDate("2026-10-19T11:00:00Z"), text: "Seen it too")])
    #expect(note.resolved == TestNote.Resolved(by: "Boris", time: try remoteDate("2026-10-19T12:00:00Z")))
    #expect(note.attachments == [.file("shot.png"), .link(URL(string: "https://github.com/o/r/issues/1")!)])
  }

  @Test func whatTheMacMayAddLaterReadsAsTheTestAndIsLeftOut() {
    #expect(TestNote(summary(anchor: "folder", block: nil)).anchor == .folder)
    #expect(TestNote(summary(anchor: "test", block: nil)).anchor == .test)
    #expect(TestNote(summary(anchor: "whiteboard")).anchor == .test)
    #expect(TestNote(summary(anchor: "block", block: nil)).anchor == .test)
    let note = TestNote(
      summary(attachments: [
        JEVisionMobile.NoteAttachmentSummary(kind: "snippet", value: "x"),
        JEVisionMobile.NoteAttachmentSummary(kind: "link", value: "javascript:alert(1)"),
        JEVisionMobile.NoteAttachmentSummary(kind: "link", value: "tel:+100"),
        JEVisionMobile.NoteAttachmentSummary(kind: "link", value: "https://example.com"),
      ]))
    #expect(note.attachments == [.link(URL(string: "https://example.com")!)])
  }

  @Test func aTimeTheMacDoesNotKnowIsLeftEmpty() {
    #expect(TestNote(summary(time: "")).time == nil)
    #expect(TestNote(summary(time: "yesterday")).time == nil)
  }

  @Test func aTestCarriesItsNotesAndOldMacsSendNone() {
    let with = JEVisionMobile.TestSummary(
      name: "Sign in", description: nil, priority: nil, testType: nil, status: nil, author: nil, app: "com.example.app", start: "open",
      width: 1, height: 1, blocks: [], frames: [], edges: [], rows: [], notes: [summary()], revision: "",
      palette: [])
    #expect(TestFlow(with).notes.map(\.id) == ["n"])
    var without = with
    without.notes = []
    #expect(TestFlow(without).notes.isEmpty)
  }
}

struct NoteTextTests {
  private func note(
    text: String = "Check the **login**", author: String = "Anna", replyCount: Int = 0, resolved: Bool = false,
    attachments: [TestNote.Attachment] = [], anchor: TestNote.Anchor = .test, area: CGRect = CGRect(x: 0, y: 0, width: 224, height: 144)
  ) -> TestNote {
    TestNote(
      id: "n", text: text, author: author, time: nil, anchor: anchor, area: area, replyCount: replyCount, replies: [],
      resolved: resolved ? TestNote.Resolved(by: "Boris", time: nil) : nil, attachments: attachments)
  }

  @Test func markdownIsShownAndItsMarksAreNot() {
    let text = NoteText.markdown("Check the **login** and `x`")
    #expect(String(text.characters) == "Check the login and x")
    #expect(text.runs.contains { $0.inlinePresentationIntent?.contains(.stronglyEmphasized) == true })
    #expect(NoteText.plain("**Hi** _there_ [docs](https://example.com)") == "Hi there docs")
    // Line breaks stay.
    #expect(String(NoteText.markdown("one\ntwo").characters) == "one\ntwo")
  }

  @Test func onlyWebLinksStayLinks() {
    let text = NoteText.markdown("[web](https://example.com) [bad](javascript:alert(1)) [call](tel:+100) [file](file:///etc/passwd) <https://example.org>")
    let links = text.runs.compactMap(\.link)
    #expect(links == [URL(string: "https://example.com")!, URL(string: "https://example.org")!])
    #expect(String(text.characters).contains("bad"))
    // On the canvas the whole card is the button, so no text is a link there.
    #expect(NoteText.markdown("[web](https://example.com)", links: false).runs.compactMap(\.link).isEmpty)
  }

  @Test func onlyWebAddressesOpen() {
    #expect(NoteText.webAddress("https://example.com/a?b=1") != nil)
    #expect(NoteText.webAddress("HTTP://example.com") != nil)
    for bad in ["javascript:alert(1)", "tel:+100", "file:///etc/passwd", "ftp://example.com", "https://", "https:///x", "example.com", "", "jevision://pair"] {
      #expect(NoteText.webAddress(bad) == nil, "\(bad)")
    }
  }

  @Test func voiceOverReadsAuthorTextAndRepliesAsOne() {
    let spoken = NoteText.spoken(note(replyCount: 3, resolved: true, attachments: [.file("a.png")]))
    #expect(
      spoken
        == [
          String(localized: "Note by \("Anna")"), "Check the login", String(localized: "\(1) files and links"),
          String(localized: "\(3) replies"), String(localized: "Resolved"),
        ].joined(separator: ". "))
    let plain = NoteText.spoken(note(text: "Just text", author: ""))
    #expect(plain == [String(localized: "Note"), "Just text"].joined(separator: ". "))
  }

  @Test func namesBecomeInitials() {
    #expect(NoteText.initials("Anna Petrova") == "AP")
    #expect(NoteText.initials("anna") == "A")
    #expect(NoteText.initials("Anna  Maria Petrova") == "AM")
    #expect(NoteText.initials("") == "")
  }

  @Test func resolvedNotesCanBeHidden() {
    let flow = TestFlow(
      name: "T", description: nil, app: "a", start: "s", size: .zero, blocks: [], frames: [], edges: [],
      notes: [note(), note(resolved: true)])
    #expect(flow.hasResolvedNotes)
    #expect(flow.notes(hidingResolved: false).count == 2)
    #expect(flow.notes(hidingResolved: true).count == 1)
    #expect(flow.notes(hidingResolved: true).allSatisfy { $0.resolved == nil })
    var open = flow
    open.notes = [note()]
    #expect(!open.hasResolvedNotes)
  }

  @Test func aNoteNamesWhatItIsAbout() {
    let click = TestFlow.Block(id: "c", kind: "click", title: "", area: nil)
    let flow = TestFlow(name: "T", description: nil, app: "a", start: "c", size: .zero, blocks: [click], frames: [], edges: [])
    #expect(NoteText.subject(of: note(anchor: .test), in: flow) == nil)
    #expect(NoteText.subject(of: note(anchor: .folder), in: flow) == String(localized: "About the folder"))
    #expect(NoteText.subject(of: note(anchor: .block("c")), in: flow) == String(localized: "About the block \(BlockInfo.title("click"))"))
    #expect(NoteText.subject(of: note(anchor: .block("gone")), in: flow) == nil)
  }

  @Test func theLineToABlockLeavesTheFacingSides() {
    let block = CGRect(x: 100, y: 100, width: 100, height: 60)
    let left = NoteLink.ends(note: CGRect(x: 0, y: 100, width: 50, height: 40), block: block)
    #expect((left.0, left.1) == (CGPoint(x: 50, y: 120), CGPoint(x: 100, y: 130)))
    let right = NoteLink.ends(note: CGRect(x: 250, y: 100, width: 50, height: 40), block: block)
    #expect((right.0, right.1) == (CGPoint(x: 250, y: 120), CGPoint(x: 200, y: 130)))
    let above = NoteLink.ends(note: CGRect(x: 120, y: 0, width: 50, height: 40), block: block)
    #expect((above.0, above.1) == (CGPoint(x: 145, y: 40), CGPoint(x: 150, y: 100)))
    let below = NoteLink.ends(note: CGRect(x: 120, y: 200, width: 50, height: 40), block: block)
    #expect((below.0, below.1) == (CGPoint(x: 145, y: 200), CGPoint(x: 150, y: 160)))
  }
}
