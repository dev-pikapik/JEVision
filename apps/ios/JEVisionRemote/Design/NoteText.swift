import CoreGraphics
import Foundation

/// A sticky note in words: its Markdown, the links it may open, and what VoiceOver reads.
enum NoteText {
  /// `text` as Markdown. Line breaks stay and block syntax such as lists shows as written.
  /// A link that is not a web address loses its link, so nothing but a web page can open from
  /// a note. `links: false` leaves no links at all, for text the surrounding card is tapped by.
  static func markdown(_ text: String, links: Bool = true) -> AttributedString {
    var result =
      (try? AttributedString(
        markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
      ?? AttributedString(text)
    var unsafe: [Range<AttributedString.Index>] = []
    for (link, range) in result.runs[\.link] {
      if let link, !links || webAddress(link) == nil { unsafe.append(range) }
    }
    for range in unsafe { result[range].link = nil }
    return result
  }

  /// `text` without its Markdown marks.
  static func plain(_ text: String) -> String {
    String(markdown(text, links: false).characters)
  }

  /// `address` when it is a web page: `http` or `https` with a host.
  static func webAddress(_ address: String) -> URL? {
    URL(string: address).flatMap(webAddress)
  }

  static func webAddress(_ url: URL) -> URL? {
    guard let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https",
      let host = url.host(percentEncoded: false), !host.isEmpty
    else { return nil }
    return url
  }

  /// What VoiceOver reads for a note: who wrote it, the text, how many replies it has and
  /// whether it is resolved.
  static func spoken(_ note: TestNote) -> String {
    var parts = [
      note.author.isEmpty ? String(localized: "Note") : String(localized: "Note by \(note.author)"),
      plain(note.text),
    ]
    if !note.attachments.isEmpty { parts.append(String(localized: "\(note.attachments.count) files and links")) }
    if note.replyCount > 0 { parts.append(String(localized: "\(note.replyCount) replies")) }
    if note.resolved != nil { parts.append(String(localized: "Resolved")) }
    return parts.filter { !$0.isEmpty }.joined(separator: ". ")
  }

  /// What a note is about in words: the block it sits at or the folder; `nil` for the test itself
  /// and for a block the test no longer has.
  static func subject(of note: TestNote, in flow: TestFlow) -> String? {
    switch note.anchor {
    case .test: nil
    case .folder: String(localized: "About the folder")
    case .block(let id):
      flow.blocks.first { $0.id == id }.map { String(localized: "About the block \(BlockInfo.title($0.kind))") }
    }
  }

  /// The two letters of a name in a round badge: `Anna Petrova` is `AP`, `anna` is `A`.
  static func initials(_ name: String) -> String {
    String(name.split(separator: " ").prefix(2).compactMap(\.first).map { String($0).uppercased() }.joined())
  }
}

extension TestFlow {
  /// The notes to draw: all of them, or only those whose discussion is still open.
  func notes(hidingResolved: Bool) -> [TestNote] {
    hidingResolved ? notes.filter { $0.resolved == nil } : notes
  }

  /// Whether any note is settled, so there is something to hide.
  var hasResolvedNotes: Bool { notes.contains { $0.resolved != nil } }
}

/// The dashed line from a note to the block it is about.
enum NoteLink {
  /// Its ends: the middle of the sides of `note` and `block` that face each other.
  static func ends(note: CGRect, block: CGRect) -> (CGPoint, CGPoint) {
    if note.maxX <= block.minX {
      (CGPoint(x: note.maxX, y: note.midY), CGPoint(x: block.minX, y: block.midY))
    } else if note.minX >= block.maxX {
      (CGPoint(x: note.minX, y: note.midY), CGPoint(x: block.maxX, y: block.midY))
    } else if note.maxY <= block.minY {
      (CGPoint(x: note.midX, y: note.maxY), CGPoint(x: block.midX, y: block.minY))
    } else {
      (CGPoint(x: note.midX, y: note.minY), CGPoint(x: block.midX, y: block.maxY))
    }
  }
}
