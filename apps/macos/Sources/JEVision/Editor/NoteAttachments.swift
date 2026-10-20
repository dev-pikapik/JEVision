import JEVisionCore
import LinkPresentation
import QuickLook
import QuickLookThumbnailing
import SwiftUI
import UniformTypeIdentifiers

/// The files and links of a note, and what adds, opens and takes them off.
struct NoteAttachments: View {
  @Environment(ProjectStore.self) private var project
  let document: FlowDocument
  let note: Note
  @State private var preview: URL?
  @State private var isChoosing = false
  @State private var isAddingLink = false
  @State private var address = ""
  @State private var addressIsWrong = false
  @State private var large: LargeFiles?
  @State private var problem: String?

  /// Files over the size Settings calls large that Git LFS doesn't take yet, waiting for a say.
  private struct LargeFiles: Equatable {
    let files: [URL]
    let size: Int64
    let lfs: ProjectGit.LFS
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      ForEach(note.attachments, id: \.self) { attachment in
        switch attachment.kind {
        case .file:
          FileRow(
            name: attachment.value, url: NoteFiles.url(of: attachment.value, note: note.id, test: document.url),
            open: { preview = $0 }, remove: { detach(attachment) })
        case .link:
          LinkRow(address: attachment.value, remove: { detach(attachment) })
        }
      }
      HStack {
        Menu {
          Button("Attach File…", systemImage: "doc.badge.plus") { isChoosing = true }
          Button("Add Link…", systemImage: "link.badge.plus") { isAddingLink = true }
        } label: {
          Label("Attach", systemImage: "paperclip")
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .help(Text("Attach a file or a link to the note"))
      }
      if isAddingLink { linkField }
    }
    .fileImporter(isPresented: $isChoosing, allowedContentTypes: [.data], allowsMultipleSelection: true) { result in
      if case .success(let urls) = result { add(urls) }
    }
    .dropDestination(for: URL.self) { urls, _ in
      add(urls)
      return !urls.isEmpty
    }
    .quickLookPreview($preview)
    .confirmationDialog(
      Text("Attach a large file?"),
      isPresented: Binding(get: { large != nil }, set: { if !$0 { large = nil } }), presenting: large
    ) { batch in
      if batch.lfs == .available {
        Button("Attach and Track with Git LFS") { attach(batch.files, tracking: true) }
      }
      Button("Attach Anyway") { attach(batch.files, tracking: false) }
      Button("Cancel", role: .cancel) {}
    } message: { batch in
      Text(Self.warning(batch.files, size: batch.size, lfs: batch.lfs))
    }
    .alert(
      Text("The file couldn’t be attached"),
      isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } })
    ) {
      Button("OK") {}
    } message: {
      Text(problem ?? "")
    }
  }

  private var linkField: some View {
    VStack(alignment: .leading, spacing: 6) {
      HStack {
        TextField("Web address", text: $address, prompt: Text(verbatim: "https://"))
          .textFieldStyle(.roundedBorder)
          .onSubmit(addLink)
          .onChange(of: address) { addressIsWrong = false }
        Button("Add", action: addLink)
          .disabled(address.trimmingCharacters(in: .whitespaces).isEmpty)
        Button("Cancel") {
          isAddingLink = false
          address = ""
          addressIsWrong = false
        }
      }
      if addressIsWrong {
        Label("Write a web address that starts with http:// or https://.", systemImage: "exclamationmark.triangle.fill")
          .font(.callout)
          .foregroundStyle(.orange)
      }
    }
  }

  private func addLink() {
    let text = address.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !text.isEmpty else { return }
    guard noteIsWebAddress(address: text) else {
      addressIsWrong = true
      return
    }
    document.attach(Attachment(kind: .link, value: text), to: note.id)
    isAddingLink = false
    address = ""
  }

  private func detach(_ attachment: Attachment) {
    document.detach(attachment, from: note.id)
  }

  /// Attaches the dropped or chosen `urls`: web pages as links, files as copies next to the test.
  /// Large files wait for the person to say what to do about them.
  private func add(_ urls: [URL]) {
    for url in urls where !url.isFileURL && noteIsWebAddress(address: url.absoluteString) {
      document.attach(Attachment(kind: .link, value: url.absoluteString), to: note.id)
    }
    let files = urls.filter(\.isFileURL)
    guard !files.isEmpty else { return }
    Task {
      var small: [URL] = []
      var big: [URL] = []
      var lfs = ProjectGit.LFS.tracked
      for file in files {
        if (try? file.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true {
          problem = String(localized: "“\(file.lastPathComponent)” is a folder. Attach the files in it instead.")
          continue
        }
        guard NoteFiles.size(of: file) > NoteFiles.largeSize else {
          small.append(file)
          continue
        }
        switch await lfsState(of: file) {
        case .tracked: small.append(file)
        case .available:
          big.append(file)
          if lfs == .tracked { lfs = .available }
        case .missing:
          big.append(file)
          lfs = .missing
        }
      }
      attach(small, tracking: false)
      if !big.isEmpty { large = LargeFiles(files: big, size: big.reduce(0) { $0 + NoteFiles.size(of: $1) }, lfs: lfs) }
    }
  }

  /// What Git LFS does about the copy of `file` that would be made next to the test.
  private func lfsState(of file: URL) async -> ProjectGit.LFS {
    guard let root = project.rootURL,
      let copy = try? NoteFiles.destination(for: file, note: note.id, test: document.url),
      RelativePath.isInside(copy, folder: root)
    else { return .missing }
    return await project.git.lfs(for: RelativePath.from(root, to: copy))
  }

  private func attach(_ files: [URL], tracking: Bool) {
    guard !files.isEmpty else { return }
    Task {
      for file in files {
        let scoped = file.startAccessingSecurityScopedResource()
        defer { if scoped { file.stopAccessingSecurityScopedResource() } }
        do {
          let copy = try NoteFiles.destination(for: file, note: note.id, test: document.url)
          if tracking, let root = project.rootURL {
            try await project.git.trackWithLFS(RelativePath.from(root, to: copy))
          }
          try NoteFiles.copy(file, to: copy)
          document.attach(Attachment(kind: .file, value: copy.lastPathComponent), to: note.id)
        } catch {
          problem = GitText.failure(error)
        }
      }
    }
  }

  private static func warning(_ files: [URL], size: Int64, lfs: ProjectGit.LFS) -> String {
    let total = ByteCountFormatter.string(fromByteCount: size, countStyle: .file)
    let limit = ByteCountFormatter.string(fromByteCount: NoteFiles.largeSize, countStyle: .file)
    let what =
      files.count == 1
      ? String(localized: "“\(files[0].lastPathComponent)” is \(total).")
      : String(localized: "\(files.count) files are over \(limit), \(total) together.")
    let advice =
      lfs == .available
      ? String(
        localized:
          "Large files make the project slow to download. Git LFS keeps them out of the project’s history; choosing it adds a line for each file to .gitattributes.")
      : String(
        localized:
          "Large files make the project slow to download. Install Git LFS, in a project that uses git, to keep them out of its history. You can change what counts as large in Settings.")
    return what + " " + advice
  }
}

// MARK: Rows

/// A file of the note: its thumbnail, name and size. Opens in Quick Look.
private struct FileRow: View {
  let name: String
  let url: URL?
  let open: (URL) -> Void
  let remove: () -> Void

  private var exists: Bool { url.map { FileManager.default.fileExists(atPath: $0.path(percentEncoded: false)) } ?? false }

  var body: some View {
    HStack(spacing: 10) {
      if let url, exists {
        FileThumbnail(url: url)
      } else {
        Image(systemName: "exclamationmark.triangle.fill")
          .foregroundStyle(.orange)
          .frame(width: 40, height: 40)
      }
      VStack(alignment: .leading, spacing: 2) {
        Text(verbatim: name)
          .lineLimit(1)
          .truncationMode(.middle)
        Text(detail)
          .font(.caption)
          .foregroundStyle(.secondary)
      }
      Spacer(minLength: 0)
      Menu {
        if let url, exists {
          Button("Quick Look", systemImage: "eye") { open(url) }
          Button("Show in Finder", systemImage: "folder") { NSWorkspace.shared.activateFileViewerSelecting([url]) }
          Divider()
        }
        Button("Remove from Note", systemImage: "minus.circle", role: .destructive, action: remove)
      } label: {
        Image(systemName: "ellipsis.circle")
      }
      .menuStyle(.borderlessButton)
      .fixedSize()
      .accessibilityLabel(Text("Actions"))
    }
    .contentShape(.rect)
    .onTapGesture { if let url, exists { open(url) } }
    .accessibilityElement(children: .combine)
    .accessibilityAddTraits(.isButton)
    .accessibilityAction(named: Text("Quick Look")) { if let url, exists { open(url) } }
    .accessibilityAction(named: Text("Remove from Note"), remove)
  }

  private var detail: String {
    guard let url, exists else {
      return String(localized: "The file is missing. It may not have been downloaded, or it was moved.")
    }
    let size = ByteCountFormatter.string(fromByteCount: NoteFiles.size(of: url), countStyle: .file)
    return switch NoteFiles.Kind(url) {
    case .image: String(localized: "Picture, \(size)")
    case .video: String(localized: "Video, \(size)")
    case .pdf: String(localized: "PDF, \(size)")
    case .other: String(localized: "File, \(size)")
    }
  }
}

/// What Quick Look shows of a file, small.
private struct FileThumbnail: View {
  let url: URL
  @State private var image: NSImage?

  var body: some View {
    Image(nsImage: image ?? NSWorkspace.shared.icon(forFile: url.path(percentEncoded: false)))
      .resizable()
      .scaledToFill()
      .frame(width: 40, height: 40)
      .clipShape(.rect(cornerRadius: 6))
      .accessibilityHidden(true)
      .task(id: url) {
        let request = QLThumbnailGenerator.Request(
          fileAt: url, size: CGSize(width: 40, height: 40), scale: NSScreen.main?.backingScaleFactor ?? 2,
          representationTypes: .thumbnail)
        image = try? await QLThumbnailGenerator.shared.generateBestRepresentation(for: request).nsImage
      }
  }
}

/// A web address of the note. One of an issue or pull request on GitHub also shows its title and
/// state when the person is signed in to GitHub; the page's own preview is fetched only on request.
private struct LinkRow: View {
  @Environment(ProjectStore.self) private var project
  let address: String
  let remove: () -> Void
  @State private var metadata: LPLinkMetadata?
  @State private var isLoading = false
  @State private var previewFailed = false

  private var url: URL? { URL(string: address) }
  private var issue: IssueRef? { noteGithubIssue(address: address) }
  private var isSignedIn: Bool {
    if case .signedIn = project.git.gitHub { true } else { false }
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      HStack(spacing: 10) {
        Image(systemName: issue == nil ? "link" : "number.circle")
          .frame(width: 40, height: 40)
          .background(.quaternary, in: .rect(cornerRadius: 6))
          .accessibilityHidden(true)
        VStack(alignment: .leading, spacing: 2) {
          if let issue {
            IssueTitle(issue: issue, isSignedIn: isSignedIn)
          } else {
            Text(verbatim: address)
              .lineLimit(2)
              .truncationMode(.middle)
          }
        }
        Spacer(minLength: 0)
        Menu {
          if let url {
            Link(destination: url) { Label("Open in Browser", systemImage: "safari") }
          }
          if issue == nil, metadata == nil {
            Button("Load Preview", systemImage: "rectangle.and.text.magnifyingglass") { Task { await loadPreview() } }
          }
          if let issue {
            Button("Refresh Status", systemImage: "arrow.clockwise") {
              Task { await NoteIssues.shared.load(issue, refresh: true) }
            }
            .disabled(!isSignedIn)
          }
          Divider()
          Button("Remove from Note", systemImage: "minus.circle", role: .destructive, action: remove)
        } label: {
          Image(systemName: "ellipsis.circle")
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .accessibilityLabel(Text("Actions"))
      }
      if isLoading {
        ProgressView().controlSize(.small)
      } else if let metadata {
        LinkPreview(metadata: metadata)
      } else if previewFailed {
        Text("The page has no preview.")
          .font(.caption)
          .foregroundStyle(.secondary)
      }
    }
    .accessibilityElement(children: .contain)
    .accessibilityAction(named: Text("Remove from Note"), remove)
  }

  private func loadPreview() async {
    guard let url else { return }
    isLoading = true
    defer { isLoading = false }
    metadata = try? await LPMetadataProvider().startFetchingMetadata(for: url)
    previewFailed = metadata == nil
  }
}

/// `owner/repo#12`, with the title and state GitHub gives for it. Asks GitHub only while signed in.
private struct IssueTitle: View {
  let issue: IssueRef
  let isSignedIn: Bool
  private var issues: NoteIssues { NoteIssues.shared }

  var body: some View {
    let name = "\(issue.owner)/\(issue.repo)#\(issue.number)"
    VStack(alignment: .leading, spacing: 2) {
      switch issues.entries[NoteIssues.key(issue)] {
      case .loaded(let status):
        Text(verbatim: status.title)
          .lineLimit(2)
        HStack(spacing: 6) {
          Text(verbatim: name)
          Label(Self.stateName(status), systemImage: Self.stateSymbol(status.state))
            .foregroundStyle(Self.stateColor(status.state))
        }
        .font(.caption)
        .foregroundStyle(.secondary)
      case .loading:
        Text(verbatim: name)
        ProgressView().controlSize(.small)
      default:
        Text(verbatim: name)
        if !isSignedIn {
          Text("Sign in to GitHub in Settings to see its title and state.")
            .font(.caption)
            .foregroundStyle(.secondary)
        } else if issues.entries[NoteIssues.key(issue)] == .unavailable {
          Text("GitHub didn’t answer for this one.")
            .font(.caption)
            .foregroundStyle(.secondary)
        }
      }
    }
    .accessibilityElement(children: .combine)
    .task(id: isSignedIn) { if isSignedIn { await issues.load(issue) } }
  }

  private static func stateName(_ status: IssueStatus) -> String {
    switch (status.state, status.isPullRequest) {
    case (.open, false): String(localized: "Open")
    case (.open, true): String(localized: "Open pull request")
    case (.closed, false): String(localized: "Closed")
    case (.closed, true): String(localized: "Closed pull request")
    case (.merged, _): String(localized: "Merged")
    }
  }

  private static func stateSymbol(_ state: IssueState) -> String {
    switch state {
    case .open: "circle.dotted"
    case .closed: "checkmark.circle"
    case .merged: "arrow.triangle.merge"
    }
  }

  private static func stateColor(_ state: IssueState) -> Color {
    switch state {
    case .open: .green
    case .closed: .red
    case .merged: .purple
    }
  }
}

/// What GitHub said about the issues and pull requests the notes point to, kept while the app
/// runs so that a note shown again doesn't ask again.
@MainActor
@Observable
final class NoteIssues {
  static let shared = NoteIssues()

  enum Entry: Equatable {
    case loading
    case loaded(IssueStatus)
    case unavailable
  }

  private(set) var entries: [String: Entry] = [:]

  static func key(_ issue: IssueRef) -> String {
    "\(issue.owner)/\(issue.repo)#\(issue.number)"
  }

  /// Asks GitHub for `issue`, unless it did already; `refresh` asks again.
  func load(_ issue: IssueRef, refresh: Bool = false) async {
    let key = Self.key(issue)
    if entries[key] == .loading { return }
    if case .loaded = entries[key], !refresh { return }
    let before = entries[key]
    entries[key] = .loading
    let result = await Task.detached {
      try? noteGithubIssueStatus(owner: issue.owner, repo: issue.repo, number: issue.number)
    }.value
    if let result {
      entries[key] = .loaded(result)
    } else if case .loaded = before {
      entries[key] = before
    } else {
      entries[key] = .unavailable
    }
  }
}

/// The preview LinkPresentation made of a web page.
private struct LinkPreview: NSViewRepresentable {
  let metadata: LPLinkMetadata

  func makeNSView(context: Context) -> LPLinkView {
    LPLinkView(metadata: metadata)
  }

  func updateNSView(_ view: LPLinkView, context: Context) {
    view.metadata = metadata
  }

  func sizeThatFits(_ proposal: ProposedViewSize, nsView: LPLinkView, context: Context) -> CGSize? {
    let size = nsView.fittingSize
    return CGSize(width: min(size.width, proposal.width ?? size.width), height: size.height)
  }
}
