import JEVisionCore
import SwiftUI

extension ProjectStore {
  /// The filter asks something: the sidebar lists, and Run All runs, only some of the tests.
  var filtersTests: Bool { !testFilterIsEmpty(filter: filter) }

  /// The tests of `items`, at any depth, that the filter chooses; every one when it asks nothing.
  func chosen(from items: [ProjectItem]) -> [ProjectItem] {
    items.chosen(by: filter, flaky: Set(flaky.keys))
  }

  /// `items` cut down to the tests the filter chooses and the folders that hold them.
  func filtered(_ items: [ProjectItem]) -> [ProjectItem] {
    filtersTests ? items.keeping(Set(chosen(from: items).map(\.id))) : items
  }

  /// Authors the tests' cards name, each once whatever its case, and the one the filter asks for.
  var authors: [String] {
    var seen = Set<String>()
    let names = items.flatMap(\.tests).compactMap { $0.card.author } + [filter.author].compactMap(\.self)
    return names.map { $0.trimmingCharacters(in: .whitespaces) }
      .filter { !$0.isEmpty && seen.insert($0.lowercased()).inserted }
      .sorted { $0.localizedCaseInsensitiveCompare($1) == .orderedAscending }
  }

  /// What Run All is called while it runs.
  var runAllTitle: String {
    filtersTests ? String(localized: "Filtered tests") : String(localized: "All tests")
  }
}

enum TestFilterText {
  /// What the filter asks, such as "High, Smoke, Flaky".
  static func summary(_ filter: TestFilter) -> String {
    [
      filter.priority.map(TestFields.title),
      filter.testType.map(TestFields.title),
      filter.status.map(TestFields.title),
      filter.author.map { String(localized: "by \($0)") },
      filter.flaky ? String(localized: "Flaky") : nil,
    ]
    .compactMap(\.self).joined(separator: ", ")
  }
}

/// The menu next to the sidebar's switch that chooses which tests the sidebar lists and Run All
/// runs, by the fields of their cards.
struct TestFilterMenu: View {
  @Environment(ProjectStore.self) private var project

  var body: some View {
    @Bindable var project = project
    let authors = project.authors
    Menu {
      Picker("Priority", selection: $project.filter.priority) {
        Text("Any").tag(Priority?.none)
        Divider()
        ForEach([Priority.critical, .high, .medium, .low], id: \.self) { Text(TestFields.title($0)).tag(Priority?.some($0)) }
      }
      Picker("Type", selection: $project.filter.testType) {
        Text("Any").tag(TestType?.none)
        Divider()
        ForEach([TestType.smoke, .regression, .acceptance], id: \.self) { Text(TestFields.title($0)).tag(TestType?.some($0)) }
      }
      Picker("Status", selection: $project.filter.status) {
        Text("Any").tag(TestStatus?.none)
        Divider()
        ForEach([TestStatus.draft, .ready, .outdated], id: \.self) { Text(TestFields.title($0)).tag(TestStatus?.some($0)) }
      }
      Picker("Author", selection: $project.filter.author) {
        Text("Anyone").tag(String?.none)
        if !authors.isEmpty { Divider() }
        ForEach(authors, id: \.self) { Text($0).tag(String?.some($0)) }
      }
      Toggle("Flaky Only", isOn: $project.filter.flaky)
      Divider()
      Button("Clear Filter") { project.filter = TestFilter() }
        .disabled(!project.filtersTests)
    } label: {
      Image(systemName: project.filtersTests ? "line.3.horizontal.decrease.circle.fill" : "line.3.horizontal.decrease.circle")
        .foregroundStyle(project.filtersTests ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
    }
    .menuStyle(.borderlessButton)
    .menuIndicator(.hidden)
    .fixedSize()
    .help(Text("Show and run only the tests that match their card: priority, type, status, author or flaky"))
    .accessibilityLabel(Text("Filter Tests"))
    .accessibilityValue(Text(TestFilterText.summary(project.filter)))
  }
}

/// Says that only some tests show and run, with the way back.
struct TestFilterBar: View {
  @Environment(ProjectStore.self) private var project

  var body: some View {
    let shown = project.chosen(from: project.items).count
    let all = project.items.reduce(0) { $0 + $1.testCount }
    HStack(spacing: 6) {
      Text("Filter: \(TestFilterText.summary(project.filter)) — showing \(shown) of \(all)")
        .lineLimit(2)
        .foregroundStyle(.secondary)
      Spacer(minLength: 0)
      Button("Clear") { project.filter = TestFilter() }
        .buttonStyle(.borderless)
    }
    .font(.caption)
    .padding(.horizontal, 12)
    .padding(.bottom, 6)
  }
}
