import AppKit
import JEVisionCore
import SwiftUI

/// Markdown as the notes of a test and of a board show it: headings, lists, check boxes, quotes
/// and inline styles, a line at a time. The text is kept as it was written; the styles are only
/// drawn.
struct MarkdownText: View {
  let text: String
  /// Names a `@name` in the text can stand for; those are drawn as mentions.
  var names: [String] = []
  var scale = 1.0

  var body: some View {
    VStack(alignment: .leading, spacing: 4 * scale) {
      ForEach(Array(Self.lines(of: text, names: names).enumerated()), id: \.offset) { _, line in
        Self.line(line, scale: scale)
      }
    }
  }

  /// The lines of `text`, with the mentions of `names` turned into links the lines draw as mentions.
  static func lines(of text: String, names: [String]) -> [String] {
    let source = names.isEmpty ? text : noteMentionsAsLinks(text: text, names: names)
    return source.components(separatedBy: "\n")
  }

  @ViewBuilder static func line(_ line: String, scale: Double = 1) -> some View {
    let trimmed = line.trimmingCharacters(in: .whitespaces)
    if let heading = trimmed.firstMatch(of: /^(#{1,6})\s+(.*)$/) {
      let styles: [NSFont.TextStyle] = [.title1, .title2, .title3, .headline, .headline, .headline]
      inline(String(heading.2)).font(.system(size: NSFont.preferredFont(forTextStyle: styles[heading.1.count - 1]).pointSize * scale, weight: .semibold))
    } else if let task = trimmed.firstMatch(of: /^[-*+]\s+\[([ xX])\]\s+(.*)$/) {
      HStack(alignment: .firstTextBaseline, spacing: 6 * scale) {
        Image(systemName: task.1 == " " ? "square" : "checkmark.square.fill")
          .foregroundStyle(task.1 == " " ? AnyShapeStyle(.secondary) : AnyShapeStyle(.tint))
        inline(String(task.2)).strikethrough(task.1 != " ")
      }
    } else if let item = trimmed.firstMatch(of: /^(?:[-*+]|\d+[.)])\s+(.*)$/) {
      HStack(alignment: .firstTextBaseline, spacing: 6 * scale) {
        Text(verbatim: trimmed.first?.isNumber == true ? String(trimmed.prefix { $0 != " " }) : "•")
          .foregroundStyle(.secondary)
        inline(String(item.1))
      }
    } else if trimmed.hasPrefix(">") {
      inline(String(trimmed.dropFirst()).trimmingCharacters(in: .whitespaces))
        .foregroundStyle(.secondary)
        .padding(.leading, 10 * scale)
        .overlay(alignment: .leading) { Capsule().fill(.quaternary).frame(width: 3 * scale) }
    } else {
      inline(line)
    }
  }

  static func inline(_ text: String) -> Text {
    Text(styled(text))
  }

  /// `text` with its inline styles. Only web addresses stay links, so a note from someone else's
  /// repository can't open a file or run a URL scheme; mentions are drawn in the accent color.
  static func styled(_ text: String) -> AttributedString {
    let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
    guard var styled = try? AttributedString(markdown: text, options: options) else { return AttributedString(text) }
    let mention = noteMentionScheme()
    let links = styled.runs.compactMap { run in run.link.map { (run.range, $0) } }
    for (range, url) in links {
      if url.scheme == mention {
        styled[range].link = nil
        styled[range].foregroundColor = .accentColor
        styled[range].inlinePresentationIntent = .stronglyEmphasized
      } else if !noteIsWebAddress(address: url.absoluteString) {
        styled[range].link = nil
      }
    }
    return styled
  }
}
