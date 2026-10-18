import JEVisionCore
import SwiftUI

/// The tests that must pass before this one runs, as tokens, and Add Test… to pick one more
/// from a search of the project's tests. In Run All and a folder's run they run first; when one
/// of them doesn't pass, this test is blocked instead of run.
struct DependenciesField: View {
  @Environment(ProjectStore.self) private var project
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
        Tokens {
          ForEach(dependsOn, id: \.self) { path in
            let name = names[RelativePath.resolve(path, in: folder)] ?? path
            Token(name: name) { save(dependsOn.filter { $0 != path }) }
              .help(Text(verbatim: path))
          }
        }
      }
    }
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

/// A test this one depends on, with a button that takes it off.
private struct Token: View {
  let name: String
  let remove: () -> Void

  var body: some View {
    HStack(spacing: 4) {
      Text(name).lineLimit(1)
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

/// Lays its views out in rows, starting a new row when the next one doesn't fit.
private struct Tokens: Layout {
  var spacing: CGFloat = 6

  func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
    let frames = layout(width: proposal.width ?? .infinity, subviews: subviews)
    return CGSize(width: frames.map(\.maxX).max() ?? 0, height: frames.map(\.maxY).max() ?? 0)
  }

  func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
    for (frame, view) in zip(layout(width: bounds.width, subviews: subviews), subviews) {
      view.place(at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY), proposal: ProposedViewSize(frame.size))
    }
  }

  private func layout(width: CGFloat, subviews: Subviews) -> [CGRect] {
    var frames: [CGRect] = []
    var point = CGPoint.zero
    var rowHeight: CGFloat = 0
    for view in subviews {
      let size = view.sizeThatFits(ProposedViewSize(width: width, height: nil))
      if point.x > 0, point.x + size.width > width {
        point = CGPoint(x: 0, y: point.y + rowHeight + spacing)
        rowHeight = 0
      }
      frames.append(CGRect(origin: point, size: CGSize(width: min(size.width, width), height: size.height)))
      point.x += size.width + spacing
      rowHeight = max(rowHeight, size.height)
    }
    return frames
  }
}
