import SwiftUI

/// The environment runs use, at the foot of the sidebar, with a menu to pick another one.
struct EnvironmentChip: View {
  @Environment(ProjectStore.self) private var project
  @State private var failure: String?

  var body: some View {
    let environments = project.environments.current
    let name = environments.selected?.name
    Menu {
      if !environments.all.isEmpty {
        Picker(selection: Binding(get: { name ?? "" }, set: { project.environments.select($0) })) {
          ForEach(environments.all, id: \.name) { Text(verbatim: $0.name).tag($0.name) }
        } label: {
          Text("Environment")
        }
        .pickerStyle(.inline)
        Divider()
      }
      if let problem = environments.problem {
        Text(problem)
      }
      Button(environments.all.isEmpty && environments.problem == nil ? "Add Environments…" : "Edit Environments…") {
        do { try project.environments.edit() } catch { failure = error.localizedDescription }
      }
    } label: {
      HStack(spacing: 6) {
        Image(systemName: environments.problem == nil ? "server.rack" : "exclamationmark.triangle.fill")
          .foregroundStyle(environments.problem == nil ? Color.secondary : Color.orange)
        Text(verbatim: name ?? String(localized: "No Environment"))
          .lineLimit(1)
          .truncationMode(.middle)
        Image(systemName: "chevron.down").imageScale(.small)
        Spacer(minLength: 0)
      }
      .font(.callout)
      .foregroundStyle(.secondary)
      .padding(.horizontal, 16)
      .padding(.vertical, 8)
      .contentShape(.rect)
    }
    .menuStyle(.button)
    .buttonStyle(.plain)
    .menuIndicator(.hidden)
    .help(Text("The environment runs use: where the test finds values such as the server address"))
    .accessibilityLabel(Text("Environment"))
    .accessibilityValue(Text(verbatim: name ?? String(localized: "No Environment")))
    .alert(
      "Can’t open the environments file", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } })
    ) {
      Button("OK") {}
    } message: {
      Text(failure ?? "")
    }
  }
}
