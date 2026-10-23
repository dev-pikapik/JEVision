import SwiftUI

/// A piece of open-source software inside the app, with its license. The list is
/// ThirdPartyLicenses.md, which scripts/licenses.py writes into the app when it is built.
struct ThirdPartyComponent: Identifiable, Hashable, Sendable {
  /// Name and version, such as `tokio 1.47.1`.
  let name: String
  let source: URL?
  let license: String
  /// The license texts it ships: one, or one per license it offers.
  let texts: [String]

  var id: String { name }

  /// The app's list, read once.
  static let inApp: [ThirdPartyComponent]? = bundled(in: .main)

  static func bundled(in bundle: Bundle) -> [ThirdPartyComponent]? {
    guard let url = bundle.url(forResource: "ThirdPartyLicenses", withExtension: "md"),
      let markdown = try? String(contentsOf: url, encoding: .utf8)
    else { return nil }
    return parse(markdown)
  }

  /// The components of the list, by name. Each `## license` heading starts a text: its components
  /// follow as `- name version — source` lines, then the text, fenced with backticks.
  static func parse(_ markdown: String) -> [ThirdPartyComponent] {
    var found: [String: (source: URL?, license: String, texts: [String])] = [:]
    var license: String?
    var entries: [Substring] = []
    var fence: Substring?
    var lines: [Substring] = []
    for line in markdown.split(separator: "\n", omittingEmptySubsequences: false) {
      if let open = fence {
        guard line == open else {
          lines.append(line)
          continue
        }
        let text = lines.joined(separator: "\n")
        for entry in entries {
          let parts = entry.split(separator: " — ", maxSplits: 1)
          let source = parts.count > 1 ? URL(string: String(parts[1])) : nil
          found[String(parts[0]), default: (source: source, license: license ?? "", texts: [])].texts.append(text)
        }
        fence = nil
      } else if line.hasPrefix("## ") {
        license = String(line.dropFirst(3))
        entries = []
      } else if line.hasPrefix("- "), license != nil {
        entries.append(line.dropFirst(2))
      } else if line.hasPrefix("```"), license != nil {
        fence = line.prefix { $0 == "`" }
        lines = []
      }
    }
    return found
      .map { ThirdPartyComponent(name: $0.key, source: $0.value.source, license: $0.value.license, texts: $0.value.texts) }
      .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
  }
}

/// The open-source software inside the app, to search, each opening its license.
struct ThirdPartyLicensesView: View {
  private let components = ThirdPartyComponent.inApp
  @State private var search = ""

  var body: some View {
    Group {
      if let components {
        let query = search.trimmingCharacters(in: .whitespaces)
        let shown =
          query.isEmpty
          ? components : components.filter { $0.name.localizedStandardContains(query) || $0.license.localizedStandardContains(query) }
        List(shown) { component in
          NavigationLink {
            ThirdPartyLicenseDetail(component: component)
          } label: {
            VStack(alignment: .leading, spacing: 2) {
              Text(verbatim: component.name)
              Text(verbatim: component.license)
                .font(.subheadline)
                .foregroundStyle(.secondary)
            }
          }
        }
        .overlay {
          if shown.isEmpty { ContentUnavailableView.search(text: query) }
        }
        .searchable(text: $search)
      } else {
        ContentUnavailableView("This copy of JEVision Remote doesn’t include the list of third-party licenses.", systemImage: "doc.text")
      }
    }
    .navigationTitle("Third-Party Licenses")
    .navigationBarTitleDisplayMode(.inline)
  }
}

private struct ThirdPartyLicenseDetail: View {
  let component: ThirdPartyComponent

  var body: some View {
    List {
      Section {
        LabeledContent("License") {
          Text(verbatim: component.license)
        }
        if let source = component.source {
          Link(destination: source) {
            LabeledContent("Source") {
              Text(verbatim: source.host() ?? source.absoluteString)
            }
          }
        }
      }
      ForEach(component.texts.indices, id: \.self) { index in
        Section {
          Text(verbatim: component.texts[index])
            .font(.footnote.monospaced())
            .textSelection(.enabled)
        }
      }
    }
    .navigationTitle(component.name)
    .navigationBarTitleDisplayMode(.inline)
  }
}
