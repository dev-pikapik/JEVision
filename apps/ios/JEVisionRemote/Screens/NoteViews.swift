import SwiftUI

/// The paper and ink of a sticky note, the Mac's colors.
private enum NoteStyle {
  static let paper = Color(
    uiColor: UIColor {
      $0.userInterfaceStyle == .dark
        ? UIColor(red: 0.29, green: 0.26, blue: 0.125, alpha: 1) : UIColor(red: 1, green: 0.965, blue: 0.78, alpha: 1)
    })
  static let ink = Color(
    uiColor: UIColor {
      $0.userInterfaceStyle == .dark
        ? UIColor(red: 1, green: 0.953, blue: 0.753, alpha: 1) : UIColor(red: 0.24, green: 0.21, blue: 0, alpha: 1)
    })
}

/// A note on the canvas: its text as far as the card reaches, who wrote it, and how the talk
/// stands. A tap opens it whole. The canvas has a fixed size, so the card does not grow with the
/// text size; the sheet it opens does.
struct NoteCard: View {
  let note: TestNote
  let open: () -> Void

  var body: some View {
    Button(action: open) {
      VStack(alignment: .leading, spacing: 6) {
        Text(NoteText.markdown(note.text, links: false))
          .font(.callout)
          .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
          .clipped()
        footer
      }
      .padding(10)
      .foregroundStyle(NoteStyle.ink)
      .background(NoteStyle.paper, in: .rect(cornerRadius: 14))
      .shadow(color: .black.opacity(0.1), radius: 4, y: 2)
      .opacity(note.resolved == nil ? 1 : 0.7)
      .contentShape(.rect(cornerRadius: 14))
    }
    .buttonStyle(.plain)
    .dynamicTypeSize(...DynamicTypeSize.xxxLarge)
    .accessibilityLabel(Text(NoteText.spoken(note)))
    .accessibilityHint(Text("Opens the note"))
    .accessibilityIdentifier("note-\(note.id)")
  }

  @ViewBuilder private var footer: some View {
    HStack(spacing: 6) {
      if !note.author.isEmpty {
        Initials(name: note.author, size: 18)
        Text(verbatim: note.author)
      }
      Spacer(minLength: 0)
      if !note.attachments.isEmpty {
        Label { Text(note.attachments.count, format: .number) } icon: { Image(systemName: "paperclip") }
      }
      if note.replyCount > 0 {
        Label { Text(note.replyCount, format: .number) } icon: { Image(systemName: "bubble.left") }
      }
      if note.resolved != nil {
        Image(systemName: "checkmark.circle.fill")
      }
    }
    .font(.caption)
    .lineLimit(1)
    .opacity(0.75)
  }
}

private struct Initials: View {
  let name: String
  let size: Double

  var body: some View {
    Text(verbatim: NoteText.initials(name))
      .font(.system(size: size * 0.5, weight: .bold))
      .frame(width: size, height: size)
      .background(NoteStyle.ink.opacity(0.15), in: .circle)
  }
}

/// A note in full: the text, what comes with it and the replies under it. Nothing here edits.
struct NoteSheet: View {
  let note: TestNote
  /// What the note is about in words, such as the block's name; `nil` for a note about the test.
  let subject: String?
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    NavigationStack {
      List {
        Section {
          VStack(alignment: .leading, spacing: 10) {
            if !note.author.isEmpty || note.time != nil || subject != nil {
              byline
            }
            Text(NoteText.markdown(note.text))
              .textSelection(.enabled)
          }
          .listRowBackground(NoteStyle.paper)
          .foregroundStyle(NoteStyle.ink)
          if let resolved = note.resolved {
            resolvedRow(resolved)
          }
        }
        if !note.attachments.isEmpty {
          Section {
            ForEach(note.attachments, id: \.self, content: attachment)
          } header: {
            Text("Files and Links")
          } footer: {
            if note.attachments.contains(where: \.isFile) { Text("Files stay on the Mac.") }
          }
        }
        if note.replyCount > 0 {
          Section {
            ForEach(note.replies) { reply in
              VStack(alignment: .leading, spacing: 4) {
                signature(author: reply.author, time: reply.time)
                  .font(.subheadline)
                Text(NoteText.markdown(reply.text))
                  .textSelection(.enabled)
              }
              .accessibilityElement(children: .combine)
            }
          } header: {
            Text("\(note.replyCount) replies")
          } footer: {
            if note.replies.count < note.replyCount {
              Text("Showing \(note.replies.count) of \(note.replyCount) replies")
            }
          }
        }
      }
      .navigationTitle(Text("Note"))
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .confirmationAction) {
          Button("Done") { dismiss() }.accessibilityIdentifier("noteDone")
        }
      }
      // Only web pages open, whatever the text links to.
      .environment(\.openURL, OpenURLAction { NoteText.webAddress($0) == nil ? .discarded : .systemAction })
    }
    .presentationDetents([.medium, .large])
  }

  private var byline: some View {
    VStack(alignment: .leading, spacing: 2) {
      signature(author: note.author, time: note.time, badge: 28)
      if let subject {
        Text(verbatim: subject).font(.subheadline).opacity(0.75)
      }
    }
    .accessibilityElement(children: .combine)
  }

  /// Who wrote it and when, on one line when it fits and one under the other when the text size
  /// leaves no room for both.
  private func signature(author: String, time: Date?, badge: Double? = nil) -> some View {
    ViewThatFits(in: .horizontal) {
      HStack(spacing: 8) { signatureParts(author, time, badge) }
        .fixedSize(horizontal: true, vertical: false)
      VStack(alignment: .leading, spacing: 2) { signatureParts(author, time, badge) }
    }
  }

  @ViewBuilder private func signatureParts(_ author: String, _ time: Date?, _ badge: Double?) -> some View {
    if !author.isEmpty {
      HStack(spacing: 8) {
        if let badge { Initials(name: author, size: badge) }
        Text(verbatim: author).bold()
      }
    }
    if let time {
      Text(time, format: .dateTime.month(.abbreviated).day().hour().minute()).opacity(0.75)
    }
  }

  private func resolvedRow(_ resolved: TestNote.Resolved) -> some View {
    Label {
      if resolved.by.isEmpty { Text("Resolved") } else { Text("Resolved by \(resolved.by)") }
    } icon: {
      Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
    }
  }

  @ViewBuilder private func attachment(_ attachment: TestNote.Attachment) -> some View {
    switch attachment {
    case .file(let name):
      LabeledContent {
        Text("On the Mac")
      } label: {
        Label(name, systemImage: "doc")
      }
      .accessibilityElement(children: .combine)
    case .link(let url):
      Link(destination: url) {
        Label(url.absoluteString, systemImage: "link")
          .lineLimit(3)
          .truncationMode(.middle)
      }
    }
  }
}

extension TestNote.Attachment {
  var isFile: Bool {
    if case .file = self { true } else { false }
  }
}
