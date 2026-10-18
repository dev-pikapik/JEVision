import JEVisionCore
import SwiftUI

/// The open pull requests of the project's repository on GitHub. Picking one shows the tests
/// and pictures it changes, each as it was and as it would be.
struct PullRequestsView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.openURL) private var openURL

  @State private var pulls: [PullRequest]?
  @State private var failure: String?
  @State private var selection: UInt64?
  @State private var isComposing = false

  private var github: String? { project.git.status?.github }
  private var pull: PullRequest? { pulls?.first { $0.number == selection } }

  var body: some View {
    Group {
      if project.rootURL == nil {
        ContentUnavailableView(
          "No project is open", systemImage: "folder",
          description: Text("Open the folder with your tests to see its pull requests."))
      } else if github == nil {
        ContentUnavailableView(
          "This project isn’t on GitHub", systemImage: "arrow.triangle.pull",
          description: Text("Pull requests appear here once the project’s repository is on GitHub."))
      } else {
        HSplitView {
          list
            .frame(minWidth: 260, idealWidth: 300, maxWidth: 400)
          detail
            .frame(minWidth: 420, maxWidth: .infinity, maxHeight: .infinity)
        }
      }
    }
    .frame(minWidth: 760, minHeight: 480)
    .navigationTitle("Pull Requests")
    .navigationSubtitle(github.map { Text(verbatim: $0) } ?? Text(verbatim: ""))
    .toolbar {
      if github != nil {
        Button("Refresh", systemImage: "arrow.clockwise") { Task { await load() } }
          .help(Text("Ask GitHub again"))
        Button("New Pull Request…", systemImage: "plus") { isComposing = true }
          .help(Text("Propose the changes on your branch"))
          .disabled(project.git.status?.hasCommits != true || project.git.status?.branch == nil)
      }
    }
    .task(id: [project.rootURL?.path(percentEncoded: false), github]) { await load() }
    .sheet(isPresented: $isComposing) {
      NewPullRequestView { _ in Task { await load() } }
    }
  }

  @ViewBuilder private var list: some View {
    if let pulls {
      if pulls.isEmpty {
        ContentUnavailableView(
          "No open pull requests", systemImage: "arrow.triangle.pull",
          description: Text("Pull requests opened on GitHub appear here."))
      } else {
        List(pulls, id: \.number, selection: $selection) { pull in
          VStack(alignment: .leading, spacing: 2) {
            Text(pull.title)
              .font(.headline)
              .lineLimit(2)
            Text(verbatim: "#\(pull.number) · \(pull.head) → \(pull.base)")
              .foregroundStyle(.secondary)
              .lineLimit(1)
            HStack(spacing: 4) {
              if pull.draft {
                Text("Draft").bold()
              }
              Text("\(pull.author) · \(Date(timeIntervalSince1970: TimeInterval(pull.updated)), format: .relative(presentation: .named))")
            }
            .font(.caption)
            .foregroundStyle(.tertiary)
          }
          .padding(.vertical, 2)
          .accessibilityElement(children: .combine)
          .contextMenu {
            Button("Open on GitHub") { open(pull) }
          }
        }
      }
    } else if let failure {
      ContentUnavailableView {
        Label("Can’t show pull requests", systemImage: "exclamationmark.triangle")
      } description: {
        Text(failure)
      } actions: {
        Button("Try Again") { Task { await load() } }
      }
    } else {
      ProgressView()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityLabel(Text("Asking GitHub"))
    }
  }

  @ViewBuilder private var detail: some View {
    if let pull {
      VStack(alignment: .leading, spacing: 0) {
        HStack(alignment: .firstTextBaseline) {
          Text(pull.title)
            .font(.title3.bold())
            .lineLimit(2)
            .accessibilityAddTraits(.isHeader)
          Spacer()
          Button("Open on GitHub") { open(pull) }
        }
        .padding([.horizontal, .top])
        PullRequestChangesView(pull: pull)
          .id(pull)
      }
    } else if pulls?.isEmpty == false {
      ContentUnavailableView(
        "Select a pull request", systemImage: "arrow.triangle.pull", description: Text("See what it changes."))
    } else {
      Color.clear
    }
  }

  private func open(_ pull: PullRequest) {
    if let url = URL(string: pull.url) { openURL(url) }
  }

  private func load() async {
    guard github != nil, let root = project.rootURL?.path(percentEncoded: false) else { return }
    failure = nil
    let result = await Task.detached(priority: .userInitiated) { Result { try githubPullRequests(project: root) } }.value
    guard !Task.isCancelled else { return }
    switch result {
    case .success(let found):
      pulls = found
      if !found.contains(where: { $0.number == selection }) { selection = found.first?.number }
    case .failure(let error):
      pulls = nil
      failure = GitText.failure(error)
    }
  }
}

/// The files of the project one pull request changes, tests first, and what changed in the
/// chosen one.
private struct PullRequestChangesView: View {
  @Environment(ProjectStore.self) private var project
  let pull: PullRequest

  @State private var changes: PullRequestChanges?
  @State private var failure: String?
  @State private var file: String?

  private var files: [FileChange] {
    let files = changes?.files ?? []
    return files.filter { GitText.isTest($0.path) } + files.filter { !GitText.isTest($0.path) }
  }

  var body: some View {
    Group {
      if let changes {
        if let change = files.first(where: { $0.path == file }) ?? files.first {
          VStack(alignment: .leading, spacing: 0) {
            HStack {
              Text(GitText.summary(files))
                .foregroundStyle(.secondary)
              Spacer()
              if files.count > 1 {
                Picker("File", selection: Binding(get: { change.path }, set: { file = $0 })) {
                  ForEach(files, id: \.path) { file in
                    Label(GitText.fileName(file.path), systemImage: GitText.symbol(file.kind)).tag(file.path)
                  }
                }
                .fixedSize()
              }
            }
            .padding()
            ChangePreview(change: change, before: changes.before, after: changes.after)
              .id([changes.after, change.path])
          }
        } else {
          ContentUnavailableView(
            "Nothing changes in this project", systemImage: "checkmark.circle",
            description: Text("This pull request changes only files outside “\(project.name)”."))
        }
      } else if let failure {
        ContentUnavailableView {
          Label("Can’t show the changes", systemImage: "exclamationmark.triangle")
        } description: {
          Text(failure)
        } actions: {
          Button("Try Again") { Task { await load() } }
        }
      } else {
        ProgressView("Downloading the changes…")
          .frame(maxWidth: .infinity, maxHeight: .infinity)
      }
    }
    .task { await load() }
  }

  private func load() async {
    guard let root = project.rootURL?.path(percentEncoded: false) else { return }
    failure = nil
    let pull = pull
    let result = await Task.detached(priority: .userInitiated) {
      Result { try githubPullRequestChanges(project: root, pullRequest: pull) }
    }.value
    guard !Task.isCancelled else { return }
    switch result {
    case .success(let found): changes = found
    case .failure(let error): failure = GitText.failure(error)
    }
  }
}
