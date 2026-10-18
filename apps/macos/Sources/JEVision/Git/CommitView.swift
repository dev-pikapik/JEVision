import JEVisionCore
import SwiftUI

/// Picks the changes to commit, shows what each one changed, and commits them with the user's
/// own words.
struct CommitView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismissWindow) private var dismissWindow
  /// Changes left out of the commit; everything else goes in, new changes too.
  @State private var excluded: Set<String> = []
  @State private var focused: String?
  @State private var message = ""
  @State private var name = ""
  @State private var email = ""
  /// Git doesn't know who commits yet: ask once, it remembers.
  @State private var asksIdentity = false

  private var changes: [FileChange] { project.git.status?.changes ?? [] }
  private var included: [FileChange] { changes.filter { !excluded.contains($0.path) } }
  private var isCommitting: Bool { project.git.activity == .committing }

  private var canCommit: Bool {
    !included.isEmpty && !message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
      && (!asksIdentity || (!name.trimmingCharacters(in: .whitespaces).isEmpty && email.contains("@")))
      && project.git.activity == nil
  }

  var body: some View {
    Group {
      if changes.isEmpty {
        ContentUnavailableView(
          "Nothing to commit", systemImage: "checkmark.circle",
          description: Text("Every change in this project is already committed."))
      } else {
        HSplitView {
          list
            .frame(minWidth: 240, idealWidth: 280, maxWidth: 380)
          preview
            .frame(minWidth: 380, maxWidth: .infinity, maxHeight: .infinity)
        }
        .safeAreaInset(edge: .bottom, spacing: 0) { composer }
      }
    }
    .frame(minWidth: 720, minHeight: 480)
    .navigationTitle(project.name.isEmpty ? Text("Commit Changes") : Text("Commit Changes in \(project.name)"))
    .task {
      project.git.refresh()
      let identity = await project.git.identity()
      name = identity.name
      email = identity.email
      asksIdentity = identity.name.isEmpty || identity.email.isEmpty
    }
    .onChange(of: changes.map(\.path), initial: true) { _, paths in
      excluded.formIntersection(paths)
      if focused.map(paths.contains) != true { focused = paths.first }
    }
    .alert(
      "Couldn’t commit", isPresented: Binding(
        get: { project.git.failure?.activity == .committing }, set: { if !$0 { project.git.failure = nil } }),
      presenting: project.git.failure
    ) { _ in
      Button("OK") {}
    } message: { Text($0.message) }
  }

  private var list: some View {
    List(selection: $focused) {
      Section {
        ForEach(changes, id: \.path) { change in
          HStack(spacing: 8) {
            Toggle(isOn: Binding(
              get: { !excluded.contains(change.path) },
              set: { if $0 { excluded.remove(change.path) } else { excluded.insert(change.path) } }
            )) {
              Text("Include \(GitText.fileName(change.path))")
            }
            .labelsHidden()
            .toggleStyle(.checkbox)
            Image(systemName: GitText.symbol(change.kind))
              .foregroundStyle(GitText.color(change.kind))
              .accessibilityLabel(Text(GitText.title(change.kind)))
            VStack(alignment: .leading, spacing: 1) {
              Text(GitText.fileName(change.path))
                .lineLimit(1)
              Text(location(change))
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
            }
          }
          .tag(change.path)
        }
      } header: {
        HStack {
          Text(GitText.summary(changes))
          Spacer()
          Button(excluded.isEmpty ? "Select None" : "Select All") {
            excluded = excluded.isEmpty ? Set(changes.map(\.path)) : []
          }
          .buttonStyle(.link)
          .font(.caption)
        }
      }
    }
  }

  /// The folder a file is in, or where a renamed file came from.
  private func location(_ change: FileChange) -> String {
    if let from = change.from { return String(localized: "Was \(from)") }
    let folder = (change.path as NSString).deletingLastPathComponent
    return folder.isEmpty ? GitText.title(change.kind) : "\(GitText.title(change.kind)) · \(folder)"
  }

  @ViewBuilder private var preview: some View {
    if let change = changes.first(where: { $0.path == focused }) {
      ChangePreview(change: change, before: project.git.status?.hasCommits == true ? "HEAD" : nil, after: nil)
        .id(change.path)
    } else {
      ContentUnavailableView("Select a change", systemImage: "doc.text.magnifyingglass")
    }
  }

  private var composer: some View {
    VStack(alignment: .leading, spacing: 10) {
      if asksIdentity {
        HStack {
          TextField("Name", text: $name, prompt: Text("Your name"))
            .textContentType(.name)
          TextField("Email", text: $email, prompt: Text(verbatim: "you@example.com"))
            .textContentType(.emailAddress)
        }
        Text("Git saves your name and email with each commit. They’re remembered for all your projects.")
          .font(.caption)
          .foregroundStyle(.secondary)
      }
      TextField("Message", text: $message, prompt: Text("Describe what changed"), axis: .vertical)
        .lineLimit(3...6)
        .textFieldStyle(.roundedBorder)
      HStack {
        Text("Commits contain only what you write. JEVision adds nothing to them.")
          .font(.caption)
          .foregroundStyle(.secondary)
        Spacer()
        if isCommitting { ProgressView().controlSize(.small) }
        Button(included.count == changes.count ? String(localized: "Commit") : String(localized: "Commit \(included.count) of \(changes.count)")) {
          commit()
        }
        .keyboardShortcut(.return, modifiers: .command)
        .buttonStyle(.borderedProminent)
        .disabled(!canCommit)
        .help(Text("Commit the checked changes (⌘↩)"))
      }
    }
    .padding()
    .background(.bar)
  }

  private func commit() {
    let identity = asksIdentity
      ? GitIdentity(name: name.trimmingCharacters(in: .whitespaces), email: email.trimmingCharacters(in: .whitespaces))
      : nil
    // A renamed file is committed under both names, so its old name goes away too.
    let paths = included.flatMap { [$0.path] + ($0.from.map { [$0] } ?? []) }
    project.git.commit(paths, message: message, as: identity) {
      message = ""
      asksIdentity = false
      dismissWindow(id: SceneID.commit)
    }
  }
}
