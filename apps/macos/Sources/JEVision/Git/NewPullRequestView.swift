import JEVisionCore
import SwiftUI

/// Proposes the current branch on GitHub with the user's own title and description, pushing
/// it first when GitHub doesn't have its latest commits.
struct NewPullRequestView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  @Environment(\.openURL) private var openURL
  /// Called with the pull request once it's open.
  var created: (PullRequest) -> Void = { _ in }

  @State private var draft: PullRequestDraft?
  @State private var loadFailure: String?
  @State private var base = ""
  @State private var title = ""
  @State private var description = ""
  @State private var made: PullRequest?

  private var git: ProjectGit { project.git }
  private var isCreating: Bool { git.activity == .proposing }

  var body: some View {
    VStack(spacing: 0) {
      if let made {
        done(made)
      } else if let draft {
        form(draft)
      } else if let loadFailure {
        ContentUnavailableView {
          Label("Can’t start a pull request", systemImage: "exclamationmark.triangle")
        } description: {
          Text(loadFailure)
        } actions: {
          Button("Try Again") { Task { await load() } }
        }
        .frame(maxHeight: .infinity)
        footer { Button("Cancel", role: .cancel) { dismiss() } }
      } else {
        ProgressView()
          .frame(maxWidth: .infinity, maxHeight: .infinity)
          .accessibilityLabel(Text("Asking GitHub"))
      }
    }
    .frame(width: 520, height: 520)
    .task { await load() }
    .alert(
      "Couldn’t create the pull request",
      isPresented: Binding(
        get: { git.failure?.activity == .proposing }, set: { if !$0 { git.failure = nil } }),
      presenting: git.failure
    ) { _ in
      Button("OK") {}
    } message: { Text($0.message) }
  }

  @ViewBuilder
  private func form(_ draft: PullRequestDraft) -> some View {
    Form {
      Section {
        LabeledContent("From branch") { Text(verbatim: draft.branch) }
        Picker("Into branch", selection: $base) {
          ForEach(draft.bases, id: \.self) { Text(verbatim: $0).tag($0) }
        }
      } footer: {
        Text("Reviewers see what changed and can merge it into the branch you choose.")
          .foregroundStyle(.secondary)
      }
      if let existing = draft.existing {
        Section {
          LabeledContent {
            Button("Open on GitHub") { open(existing) }
          } label: {
            Text("This branch already has an open pull request")
            Text(verbatim: "#\(existing.number) \(existing.title)")
          }
        }
      } else {
        Section {
          TextField("Title", text: $title, prompt: Text("What this change does"))
          TextField("Description", text: $description, prompt: Text("Why, and what to look at (optional)"), axis: .vertical)
            .lineLimit(5...10)
        } footer: {
          VStack(alignment: .leading, spacing: 6) {
            if draft.unpushed {
              Label("Your latest commits aren’t on GitHub yet. They’ll be pushed first.", systemImage: "arrow.up.circle")
            }
            if git.status?.changes.isEmpty == false {
              Label("Changes that aren’t committed won’t be included.", systemImage: "info.circle")
            }
            Text("The pull request contains only what you write. JEVision adds nothing to it.")
          }
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
        }
      }
    }
    .formStyle(.grouped)
    .disabled(isCreating)
    footer {
      if isCreating {
        ProgressView().controlSize(.small)
          .accessibilityLabel(Text(draft.unpushed ? "Pushing and creating…" : "Creating…"))
      }
      Button("Cancel", role: .cancel) { dismiss() }
      if draft.existing == nil {
        Button(draft.unpushed ? "Push and Create" : "Create Pull Request") {
          git.createPullRequest(base: base, title: title, description: description, push: draft.unpushed) {
            made = $0
            created($0)
          }
        }
        .keyboardShortcut(.defaultAction)
        .disabled(title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || base.isEmpty || git.activity != nil)
      }
    }
  }

  private func done(_ pull: PullRequest) -> some View {
    Group {
      ContentUnavailableView {
        Label("Pull request #\(pull.number) is open", systemImage: "checkmark.circle")
      } description: {
        Text(verbatim: pull.title)
      }
      .frame(maxHeight: .infinity)
      footer {
        Button("Open on GitHub") { open(pull) }
        Button("Done") { dismiss() }
          .keyboardShortcut(.defaultAction)
      }
    }
  }

  private func footer(@ViewBuilder _ buttons: () -> some View) -> some View {
    HStack {
      Spacer()
      buttons()
    }
    .padding()
  }

  private func open(_ pull: PullRequest) {
    if let url = URL(string: pull.url) { openURL(url) }
  }

  private func load() async {
    guard let root = project.rootURL?.path(percentEncoded: false) else { return }
    loadFailure = nil
    let result = await Task.detached(priority: .userInitiated) { Result { try githubPullRequestDraft(project: root) } }.value
    switch result {
    case .success(let found):
      base = found.bases.contains(found.base) ? found.base : found.bases.first ?? ""
      draft = found
    case .failure(let error):
      loadFailure = GitText.failure(error)
    }
  }
}
