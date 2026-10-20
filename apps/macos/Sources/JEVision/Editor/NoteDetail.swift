import JEVisionCore
import SwiftUI

/// A note as the inspector shows it: who wrote it and when, its text in Markdown, what it is
/// about, its files and links, the talk under it and its history.
struct NoteDetail: View {
  let document: FlowDocument
  let id: String
  @State private var isWriting = false
  @FocusState private var isFocused: Bool

  var body: some View {
    if let note = document.note(id) {
      VStack(alignment: .leading, spacing: 12) {
        header(note)
        text(note)
        about(note)
        NoteAttachments(document: document, note: note)
        NoteReplies(document: document, note: note)
        NoteHistory(document: document, note: note)
      }
      .padding(.vertical, 4)
    }
  }

  private func header(_ note: Note) -> some View {
    VStack(alignment: .leading, spacing: 8) {
      HStack(alignment: .center, spacing: 8) {
        Initials(name: note.author)
        VStack(alignment: .leading, spacing: 1) {
          Text(note.author.isEmpty ? String(localized: "Unknown author") : note.author)
            .font(.headline)
          if let date = NoteTime.date(note.time) {
            Text(date, format: .relative(presentation: .named))
              .font(.caption)
              .foregroundStyle(.secondary)
              .help(Text(date, format: .dateTime.day().month(.wide).year().hour().minute()))
          }
        }
        .accessibilityElement(children: .combine)
        Spacer(minLength: 0)
        if note.resolved == nil {
          Button("Resolve", systemImage: "checkmark.circle") { document.resolve(note.id, by: document.signature) }
            .help(Text("Mark the discussion as settled"))
        }
      }
      if let resolved = note.resolved {
        HStack(spacing: 8) {
          Label {
            Text(settled(resolved))
          } icon: {
            Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
          }
          .font(.callout)
          Spacer(minLength: 0)
          Button("Reopen") { document.reopen(note.id) }
        }
        .accessibilityElement(children: .combine)
      }
    }
  }

  private func settled(_ resolved: Resolved) -> String {
    let who = resolved.by.isEmpty ? String(localized: "Someone") : resolved.by
    guard let date = NoteTime.date(resolved.time) else { return String(localized: "Resolved by \(who)") }
    return String(localized: "Resolved by \(who), \(date.formatted(.relative(presentation: .named)))")
  }

  @ViewBuilder private func text(_ note: Note) -> some View {
    let text = Binding(get: { document.note(id)?.text ?? "" }, set: { document.editNote(id, text: $0) })
    if isWriting || note.text.isEmpty {
      VStack(alignment: .leading, spacing: 6) {
        TextEditor(text: text)
          .font(.body)
          .scrollContentBackground(.hidden)
          .focused($isFocused)
          .frame(minHeight: 84)
          .padding(4)
          .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: 8))
          .accessibilityLabel(Text("Note text"))
          .onAppear { if note.text.isEmpty { isFocused = true } }
          .onChange(of: isFocused) { if !isFocused { finishWriting() } }
        MentionSuggestions(text: text, names: document.people)
        HStack {
          Text("You can use Markdown: lists, links, bold and italic text.")
            .font(.caption)
            .foregroundStyle(.secondary)
          Spacer(minLength: 0)
          if !note.text.isEmpty { Button("Done", action: finishWriting) }
        }
      }
    } else {
      VStack(alignment: .leading, spacing: 6) {
        MarkdownText(text: note.text, names: document.people)
          .frame(maxWidth: .infinity, alignment: .leading)
          .textSelection(.enabled)
        Button("Edit Text", systemImage: "pencil") {
          isWriting = true
          isFocused = true
        }
        .buttonStyle(.link)
        .font(.callout)
      }
    }
  }

  private func finishWriting() {
    isWriting = false
    document.endUndoStep()
  }

  private func about(_ note: Note) -> some View {
    let anchor = Binding(get: { note.anchor }, set: { document.setAnchor(of: note.id, to: $0) })
    return Picker(selection: anchor) {
      Text("This test").tag(NoteAnchor.test)
      Text("This folder").tag(NoteAnchor.folder)
      Divider()
      if case .block(let id) = note.anchor, document.block(id) == nil {
        Text("A block that is gone").tag(note.anchor)
      }
      ForEach(document.order, id: \.self) { block in
        Text(verbatim: "\(document.step(of: block)). \(BlockInfo.title(document.block(block)?.kind.typeName ?? ""))")
          .tag(NoteAnchor.block(id: block))
      }
    } label: {
      Text("About")
    }
    .pickerStyle(.menu)
    .help(Text("What the note is about. A note about a block moves with it."))
  }
}

/// The time a note, reply or settled discussion carries, as a date.
enum NoteTime {
  static func date(_ time: String) -> Date? {
    try? Date(time, strategy: .iso8601)
  }
}

/// The first letters of a name in a circle.
struct Initials: View {
  let name: String
  var size = 24.0

  var body: some View {
    Text(verbatim: Self.letters(of: name))
      .font(.system(size: size * 0.4, weight: .bold))
      .frame(width: size, height: size)
      .background(.quaternary, in: .circle)
      .accessibilityHidden(true)
  }

  static func letters(of name: String) -> String {
    name.split(separator: " ").prefix(2).compactMap(\.first).map(String.init).joined().uppercased()
  }
}

/// Names to finish the `@mention` being typed at the end of `text` with.
struct MentionSuggestions: View {
  @Binding var text: String
  let names: [String]

  var body: some View {
    let found = names.isEmpty ? [] : noteMentionSuggestions(text: text, names: names)
    if !found.isEmpty {
      HStack(spacing: 6) {
        Image(systemName: "at")
          .foregroundStyle(.secondary)
          .accessibilityHidden(true)
        ForEach(found.prefix(3), id: \.self) { name in
          Button {
            text = noteCompleteMention(text: text, name: name)
          } label: {
            Text(verbatim: name).lineLimit(1)
          }
          .buttonStyle(.bordered)
          .controlSize(.small)
          .accessibilityHint(Text("Mentions this person"))
        }
      }
    }
  }
}

/// The talk under a note, and a field to add to it.
private struct NoteReplies: View {
  let document: FlowDocument
  let note: Note
  @State private var draft = ""
  @State private var editing: String?
  @State private var edited = ""

  private var isBlank: Bool { draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      if !note.replies.isEmpty {
        Text("Discussion")
          .font(.subheadline.weight(.semibold))
          .accessibilityAddTraits(.isHeader)
      }
      ForEach(note.replies, id: \.id) { reply in
        row(reply)
      }
      composer
    }
  }

  private func row(_ reply: Reply) -> some View {
    HStack(alignment: .top, spacing: 8) {
      Initials(name: reply.author)
      VStack(alignment: .leading, spacing: 3) {
        HStack(spacing: 6) {
          Text(reply.author.isEmpty ? String(localized: "Unknown author") : reply.author)
            .font(.callout.weight(.semibold))
          if let date = NoteTime.date(reply.time) {
            Text(date, format: .relative(presentation: .named))
              .font(.caption)
              .foregroundStyle(.secondary)
          }
          Spacer(minLength: 0)
          Menu {
            Button("Edit", systemImage: "pencil") {
              edited = reply.text
              editing = reply.id
            }
            Button("Delete", systemImage: "trash", role: .destructive) { document.deleteReply(reply.id, of: note.id) }
          } label: {
            Image(systemName: "ellipsis.circle")
          }
          .menuStyle(.borderlessButton)
          .fixedSize()
          .accessibilityLabel(Text("Actions"))
        }
        if editing == reply.id {
          TextEditor(text: $edited)
            .font(.body)
            .scrollContentBackground(.hidden)
            .frame(minHeight: 60)
            .padding(4)
            .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: 8))
            .accessibilityLabel(Text("Reply text"))
            .onChange(of: edited) {
              if !edited.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                document.editReply(reply.id, of: note.id, text: edited)
              }
            }
          MentionSuggestions(text: $edited, names: document.people)
          Button("Done") {
            editing = nil
            document.endUndoStep()
          }
        } else {
          MarkdownText(text: reply.text, names: document.people)
            .frame(maxWidth: .infinity, alignment: .leading)
            .textSelection(.enabled)
        }
      }
    }
    .accessibilityElement(children: .contain)
  }

  private var composer: some View {
    VStack(alignment: .leading, spacing: 6) {
      TextField("Reply", text: $draft, prompt: Text("Reply…"), axis: .vertical)
        .lineLimit(1...6)
        .textFieldStyle(.roundedBorder)
        .onSubmit(send)
        .help(Text("Press Return to send, Option-Return for a new line."))
      MentionSuggestions(text: $draft, names: document.people)
      HStack {
        Spacer(minLength: 0)
        Button("Reply", systemImage: "arrowshape.turn.up.left", action: send)
          .disabled(isBlank)
          .keyboardShortcut(.return, modifiers: .command)
      }
    }
  }

  private func send() {
    guard !isBlank else { return }
    document.reply(to: note.id, author: document.signature, text: draft)
    document.endUndoStep()
    draft = ""
  }
}

/// What happened to a note over the history of the test file in git, newest first.
private struct NoteHistory: View {
  @Environment(ProjectStore.self) private var project
  let document: FlowDocument
  let note: Note
  @State private var isOpen = false
  @State private var events: [NoteEvent]?
  @State private var failure: String?

  var body: some View {
    DisclosureGroup(isExpanded: $isOpen) {
      VStack(alignment: .leading, spacing: 10) {
        if let events {
          if events.isEmpty { alone }
          ForEach(Array(events.enumerated()), id: \.offset) { _, event in
            row(event)
          }
        } else {
          ProgressView().controlSize(.small)
        }
        if let failure {
          Text(failure)
            .font(.caption)
            .foregroundStyle(.secondary)
        }
      }
      .padding(.top, 6)
    } label: {
      Text("History")
    }
    .task(id: isOpen) { if isOpen { await load() } }
  }

  /// All there is to tell without a history: who wrote the note and when.
  private var alone: some View {
    Group {
      if let date = NoteTime.date(note.time) {
        Text("Written by \(note.author.isEmpty ? String(localized: "Someone") : note.author), \(date.formatted(.relative(presentation: .named)))")
      } else {
        Text("Git doesn’t track this project, so there is no history to show.")
      }
    }
    .font(.callout)
    .foregroundStyle(.secondary)
  }

  private func row(_ event: NoteEvent) -> some View {
    VStack(alignment: .leading, spacing: 2) {
      Text(Self.title(event))
        .font(.callout)
      if let detail = detail(event) {
        Text(verbatim: detail)
          .font(.caption)
          .foregroundStyle(.secondary)
          .lineLimit(2)
      }
      Text(when(event))
        .font(.caption2)
        .foregroundStyle(.tertiary)
    }
    .accessibilityElement(children: .combine)
  }

  private func load() async {
    guard let root = project.rootURL, RelativePath.isInside(document.url, folder: root) else {
      events = []
      return
    }
    let path = root.path(percentEncoded: false)
    let test = RelativePath.from(root, to: document.url)
    let id = note.id
    failure = nil
    do {
      events = try await Task.detached { try noteHistory(project: path, test: test, note: id) }.value
    } catch {
      events = []
      failure = GitText.failure(error)
    }
  }

  private func detail(_ event: NoteEvent) -> String? {
    switch event.kind {
    case .created, .edited, .replied, .replyEdited, .replyRemoved, .attached, .detached:
      event.detail.isEmpty ? nil : String(event.detail.prefix(160))
    case .anchoredToBlock:
      document.block(event.detail).map {
        "\(document.step(of: event.detail)). \(BlockInfo.title($0.kind.typeName))"
      }
    default: nil
    }
  }

  private func when(_ event: NoteEvent) -> String {
    guard event.time > 0 else { return String(localized: "Not committed yet") }
    let date = Date(timeIntervalSince1970: TimeInterval(event.time)).formatted(.relative(presentation: .named))
    return event.commit.isEmpty ? date : "\(date) · \(event.commit)"
  }

  private static func title(_ event: NoteEvent) -> String {
    let who = event.author.isEmpty ? String(localized: "Someone") : event.author
    return switch event.kind {
    case .created: String(localized: "\(who) wrote the note")
    case .edited: String(localized: "\(who) changed the text")
    case .anchoredToTest: String(localized: "\(who) made the note about the test")
    case .anchoredToFolder: String(localized: "\(who) made the note about the folder")
    case .anchoredToBlock: String(localized: "\(who) made the note about a block")
    case .attached: String(localized: "\(who) attached a file or link")
    case .detached: String(localized: "\(who) took off a file or link")
    case .replied: String(localized: "\(who) replied")
    case .replyEdited: String(localized: "\(who) changed a reply")
    case .replyRemoved: String(localized: "\(who) deleted a reply")
    case .resolved: String(localized: "\(who) resolved the discussion")
    case .reopened: String(localized: "\(who) reopened the discussion")
    case .removed: String(localized: "\(who) deleted the note")
    }
  }
}

/// The inspector of a selected note.
struct NoteInspector: View {
  let document: FlowDocument
  let id: String

  var body: some View {
    Form {
      Section {
        NoteDetail(document: document, id: id)
      } header: {
        Text("Note")
      }
      Section {
        Button("Delete Note", systemImage: "trash", role: .destructive) { document.remove([.note(id: id)]) }
      }
    }
    .formStyle(.grouped)
  }
}

/// The notes about a block, in the inspector of the block, and a button to add one.
struct BlockNotes: View {
  let document: FlowDocument
  let block: String

  var body: some View {
    Section {
      ForEach(document.notes(about: block), id: \.id) { note in
        NoteDetail(document: document, id: note.id)
      }
      Button("Add Note to This Block", systemImage: "note.text.badge.plus") {
        document.addNote(about: block, select: false)
      }
    } header: {
      Text("Notes")
    }
  }
}
