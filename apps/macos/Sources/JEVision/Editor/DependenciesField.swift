import JEVisionCore
import SwiftUI

/// The tests that must pass before this one runs, as tokens that open them, and Add Test… to
/// pick one more from a search of the project's tests. In Run All and a folder's run they run first; when one
/// of them doesn't pass, this test is blocked instead of run.
struct DependenciesField: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  let document: FlowDocument
  /// As saved: paths relative to the folder of the test.
  let dependsOn: [String]
  @State private var isAdding = false

  var body: some View {
    let folder = document.url.deletingLastPathComponent()
    let tests = project.items.flatMap(\.tests)
    let names = Dictionary(tests.map { ($0.url.standardizedFileURL, $0.name) }, uniquingKeysWith: { a, _ in a })
    VStack(alignment: .leading, spacing: 8) {
      LabeledContent {
        Button("Add Test…", systemImage: "plus") { isAdding = true }
          .popover(isPresented: $isAdding, arrowEdge: .trailing) {
            TestSearch(
              tests: tests.filter { test in
                test.url.standardizedFileURL != document.url.standardizedFileURL
                  && !dependsOn.contains { RelativePath.resolve($0, in: folder) == test.url.standardizedFileURL }
              }
            ) { test in
              isAdding = false
              save(dependsOn + [RelativePath.from(folder, to: test.url)])
            }
          }
      } label: {
        Text("Runs After")
        Text("Tests that must pass first. If one doesn’t, this test is blocked instead of run.")
      }
      if !dependsOn.isEmpty {
        WrapLayout {
          ForEach(dependsOn, id: \.self) { path in
            let url = RelativePath.resolve(path, in: folder)
            let name = names[url]
            Token(name: name ?? path, open: name == nil ? nil : { open(url) }) {
              save(dependsOn.filter { $0 != path })
            }
            .help(Text(verbatim: path))
          }
        }
      }
    }
  }

  /// Shows the test in the editor in place of this one.
  private func open(_ url: URL) {
    guard let root = project.rootURL else { return }
    dismiss()
    project.selection = RelativePath.from(root, to: url)
  }

  private func save(_ dependsOn: [String]) {
    document.updateFlow(field: "dependsOn") { $0.dependsOn = dependsOn }
  }
}

/// A search field over the project's tests; picking one hands it over.
private struct TestSearch: View {
  let tests: [ProjectItem]
  let pick: (ProjectItem) -> Void
  @State private var query = ""
  @FocusState private var focused: Bool

  var body: some View {
    let found = query.isEmpty ? tests : ProjectScanner.tests(in: tests, matching: query)
    VStack(alignment: .leading, spacing: 8) {
      TextField("Search tests", text: $query)
        .textFieldStyle(.roundedBorder)
        .focused($focused)
        .onSubmit { if let first = found.first { pick(first) } }
      if found.isEmpty {
        Text(tests.isEmpty ? "No other tests in the project" : "No tests match")
          .foregroundStyle(.secondary)
          .frame(maxWidth: .infinity, minHeight: 60)
      } else {
        List(found) { test in
          Button { pick(test) } label: {
            VStack(alignment: .leading, spacing: 1) {
              Text(test.name)
              if !test.parentPath.isEmpty {
                Text(test.parentPath).font(.caption).foregroundStyle(.secondary)
              }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(.rect)
          }
          .buttonStyle(.plain)
          .accessibilityHint(Text("Runs this test first"))
        }
        .listStyle(.plain)
        .frame(height: min(CGFloat(found.count) * 34 + 8, 240))
      }
    }
    .padding(12)
    .frame(width: 280)
    .onAppear { focused = true }
  }
}

/// A test this one depends on: its name opens it when it is in the project, and a button takes
/// it off.
private struct Token: View {
  let name: String
  let open: (() -> Void)?
  let remove: () -> Void

  var body: some View {
    HStack(spacing: 4) {
      if let open {
        Button(name, action: open)
          .buttonStyle(.link)
          .lineLimit(1)
          .accessibilityHint(Text("Opens it"))
      } else {
        Text(name).lineLimit(1)
      }
      Button("Remove", systemImage: "xmark.circle.fill", action: remove)
        .labelStyle(.iconOnly)
        .buttonStyle(.borderless)
        .foregroundStyle(.secondary)
        .accessibilityLabel(Text("Remove \(name)"))
    }
    .font(.callout)
    .padding(.leading, 10)
    .padding(.trailing, 5)
    .padding(.vertical, 3)
    .background(.tint.opacity(0.15), in: .capsule)
    .accessibilityElement(children: .contain)
  }
}
