import JEVisionCore
import JEVisionHands
import SwiftUI

/// The test card: what the test checks and needs, who looks after it, the tests it runs after,
/// its retries, flaky mark, quarantine, data and secrets.
struct TestDetailsView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  let document: FlowDocument
  @State private var settingSecret: SecretName?
  @State private var settingValue: EnvironmentValueName?
  @State private var addingEnvironment = false
  /// Bumped after a secret is saved, to read the statuses again.
  @State private var secretsVersion = 0

  var body: some View {
    if let flow = document.flow {
      let issues = document.issues.filter { $0.param("block") == nil }
      Form {
        Section {
          TextField(text: binding(flow.name, "name") { $0.name = $1 }) { Text("Name") }
          IssueList(issues: issues.filter { $0.param("field") == "name" })
          TextField(
            text: binding(flow.description ?? "", "description") { $0.description = $1.isEmpty ? nil : $1 },
            prompt: Text("What the test checks"), axis: .vertical
          ) { Text("Description") }
          .lineLimit(1...4)
          TextField(
            text: binding(flow.preconditions ?? "", "preconditions") { $0.preconditions = $1.isEmpty ? nil : $1 },
            prompt: Text("What must be true before it starts"), axis: .vertical
          ) { Text("Preconditions") }
          .lineLimit(1...4)
          LabeledContent("App") {
            AppPicker(
              bundleID: binding(flow.app.isEmpty ? nil : flow.app, "app") { $0.app = $1 ?? "" },
              noneTitle: Text("Choose App"))
          }
          IssueList(issues: issues.filter { $0.param("field") == "app" })
        }
        missingSection(flow)
        Section {
          CommitField(
            title: "Tags", prompt: Text(verbatim: "smoke, sign-in"), value: flow.tags.joined(separator: ", "),
            tidy: { TestFields.tags($0).joined(separator: ", ") }
          ) { text in document.updateFlow(field: "tags") { $0.tags = TestFields.tags(text) } }
          IssueList(issues: issues.filter { $0.param("field") == "tags" || $0.code == "flow.tags.duplicate" })
          Picker(selection: binding(flow.testType, "testType") { $0.testType = $1 }) {
            Text("Not Set").tag(TestType?.none)
            Divider()
            ForEach([TestType.smoke, .regression, .acceptance], id: \.self) { Text(TestFields.title($0)).tag(TestType?.some($0)) }
          } label: {
            Text("Type")
          }
          Picker(selection: binding(flow.priority, "priority") { $0.priority = $1 }) {
            Text("Not Set").tag(Priority?.none)
            Divider()
            ForEach([Priority.critical, .high, .medium, .low], id: \.self) { Text(TestFields.title($0)).tag(Priority?.some($0)) }
          } label: {
            Text("Priority")
          }
          Picker(selection: binding(flow.status, "status") { $0.status = $1 }) {
            Text("Not Set").tag(TestStatus?.none)
            Divider()
            ForEach([TestStatus.draft, .ready, .outdated], id: \.self) { Text(TestFields.title($0)).tag(TestStatus?.some($0)) }
          } label: {
            Text("Status")
          }
          CommitField(title: "Author", prompt: Text("Who looks after the test"), value: flow.author ?? "") { author in
            document.updateFlow(field: "author") { $0.author = author.isEmpty ? nil : author }
          }
          CommitField(title: "Platform", prompt: Text(verbatim: "macOS 26"), value: flow.platform ?? "") { platform in
            document.updateFlow(field: "platform") { $0.platform = platform.isEmpty ? nil : platform }
          }
          let linkIssues = issues.filter { $0.param("field") == "link" }
          LabeledContent {
            HStack {
              CommitField(title: "Link", prompt: Text(verbatim: "https://"), value: flow.link ?? "") { link in
                document.updateFlow(field: "link") { $0.link = link.isEmpty ? nil : link }
              }
              .labelsHidden()
              if linkIssues.isEmpty, let link = flow.link.flatMap(URL.init(string:)) {
                Link(destination: link) { Image(systemName: "arrow.up.forward.square") }
                  .buttonStyle(.borderless)
                  .help(Text("Open the link in the browser"))
                  .accessibilityLabel(Text("Open Link"))
              }
            }
          } label: {
            Text("Link")
            Text("The issue or requirement the test covers.")
          }
          IssueList(issues: linkIssues)
        }
        Section {
          Picker(selection: binding(flow.retries, "retries") { $0.retries = $1 }) {
            Text("Project Default (\(project.settings.retries ?? 0))").tag(UInt32?.none)
            Divider()
            ForEach(UInt32(0)...10, id: \.self) { Text($0, format: .number).tag(UInt32?.some($0)) }
          } label: {
            Text("Retries")
            Text("How many more times to run the test if it fails. The project default is set in Settings › Advanced.")
          }
          Toggle(isOn: binding(flow.flaky, "flaky") { $0.flaky = $1 }) {
            Text("Flaky")
            Text(TestFields.flakyExplanation)
          }
        }
        Section {
          DependenciesField(document: document, dependsOn: flow.dependsOn)
          IssueList(issues: issues.filter { $0.param("field") == "dependsOn" })
        }
        if let file = project.testID(document.url) {
          let runs = project.runs.filter { $0.report.file == file }
          if !runs.isEmpty {
            Section("Health") {
              HealthView(test: file, health: testHealth(runs: runs.map(\.report), nowMs: Int64(Date.now.timeIntervalSince1970 * 1000)))
            }
          }
          QuarantineSection(file: file, name: flow.name)
        }
        let files = DataColumns.files(in: flow)
        if !files.isEmpty {
          Section("Data Files") {
            ForEach(files, id: \.self) { Label($0, systemImage: "tablecells") }
          }
        }
        let secrets = requiredSecrets(flow: flow)
        let environment = project.environments.current.selected?.name
        Section {
          if secrets.isEmpty {
            Text("This test doesn’t use secrets. Add one in a Type Text block with Insert.")
              .foregroundStyle(.secondary)
          }
          ForEach(secrets, id: \.self) { name in
            SecretStatusRow(
              name: name, isSet: secretsVersion >= 0 && SecretStore.isSet(name, environment: environment)
            ) { settingSecret = SecretName(id: name) }
          }
        } header: {
          if let environment {
            Text("Secrets in \(environment)")
          } else {
            Text("Secrets")
          }
        } footer: {
          Text("Values are kept in the Keychain on this Mac or read from environment variables of the same name.")
            .foregroundStyle(.secondary)
        }
        Section {
          if project.git.status?.hasCommits == true, let path = project.selection {
            LabeledContent {
              Button("History…") {
                dismiss()
                project.historyPath = path
              }
            } label: {
              Text("History")
              Text("Every committed version of this test.")
            }
          }
          LabeledContent {
            Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([document.url]) }
          } label: {
            Text("File")
            Text(verbatim: project.testID(document.url) ?? document.url.lastPathComponent)
          }
        }
        IssueList(issues: issues.filter {
          !["name", "app", "tags", "dependsOn", "link"].contains($0.param("field") ?? "") && $0.code != "flow.tags.duplicate"
        })
      }
      .formStyle(.grouped)
      .frame(width: 400)
      .frame(minHeight: 300, maxHeight: 560)
      .sheet(item: $settingSecret) { secret in
        SetSecretSheet(name: secret.id, environment: project.environments.current.selected?.name) {
          secretsVersion += 1
        }
      }
      .sheet(item: $settingValue) { value in
        if let environment = project.environments.current.selected?.name {
          SetEnvironmentValueSheet(name: value.id, environment: environment)
        }
      }
      .sheet(isPresented: $addingEnvironment) { AddEnvironmentSheet(valueNames: requiredEnvValues(flow: flow)) }
    }
  }

  /// What a run on this Mac would lack: values of the chosen environment and secrets, each with
  /// a way to set it. Nothing is shown when the test has all it needs.
  @ViewBuilder
  private func missingSection(_ flow: Flow) -> some View {
    let missing = MissingValues(for: flow, in: project.environments.current) {
      secretsVersion >= 0 && SecretStore.isSet($0, environment: $1)
    }
    if !missing.isEmpty {
      Section {
        if let problem = missing.problem {
          Label {
            Text(problem)
          } icon: {
            Image(systemName: "exclamationmark.octagon.fill").foregroundStyle(.red)
          }
          Button("Edit Environments…") { project.environments.edit() }
        } else if let environment = missing.environment {
          ForEach(missing.values, id: \.self) { name in
            LabeledContent {
              HStack {
                Text("Not set in \(environment)").foregroundStyle(.secondary)
                Button("Set…") { settingValue = EnvironmentValueName(id: name) }
                  .accessibilityLabel(Text("Set \(name)"))
              }
            } label: {
              Label {
                Text(verbatim: name)
              } icon: {
                Image(systemName: "server.rack").foregroundStyle(.orange)
              }
            }
          }
        } else if !missing.values.isEmpty {
          ForEach(missing.values, id: \.self) { name in
            Label {
              Text(verbatim: name)
            } icon: {
              Image(systemName: "server.rack").foregroundStyle(.orange)
            }
          }
          Button("Add Environment…") { addingEnvironment = true }
        }
        ForEach(missing.secrets, id: \.self) { name in
          LabeledContent {
            HStack {
              Text("Not set").foregroundStyle(.secondary)
              Button("Set…") { settingSecret = SecretName(id: name) }
                .accessibilityLabel(Text("Set \(name)"))
            }
          } label: {
            Label {
              Text(verbatim: name)
            } icon: {
              Image(systemName: "key.fill").foregroundStyle(.orange)
            }
          }
        }
      } header: {
        Text("Missing on This Mac")
      } footer: {
        Text("A run would stop at these. Values go to the environments file; secrets stay in the Keychain on this Mac.")
          .foregroundStyle(.secondary)
      }
    }
  }

  private func binding<T>(_ value: T, _ field: String, _ set: @escaping (inout Flow, T) -> Void) -> Binding<T> {
    let document = document
    return Binding(get: { value }, set: { new in document.updateFlow(field: field) { set(&$0, new) } })
  }
}

/// One line of text saved when the field is left or Return is pressed, not on every key, so a
/// half-typed value is never checked or saved.
private struct CommitField: View {
  let title: LocalizedStringKey
  let prompt: Text
  let value: String
  /// The text as it's saved: trimmed by default.
  var tidy: (String) -> String = { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
  let save: (String) -> Void
  @State private var text = ""
  @FocusState private var focused: Bool

  var body: some View {
    TextField(text: $text, prompt: prompt) { Text(title) }
      .focused($focused)
      .onSubmit(commit)
      .onChange(of: focused) { if !focused { commit() } }
      .onDisappear(perform: commit)
      .onChange(of: value, initial: true) { if !focused { text = value } }
  }

  private func commit() {
    let new = tidy(text)
    if new != value { save(new) }
    text = new
  }
}

enum TestFields {
  /// Tags from "a, b,, a": trimmed, without empty ones and repeats, in the order typed.
  static func tags(_ text: String) -> [String] {
    var seen = Set<String>()
    return text.split(separator: ",")
      .map { $0.trimmingCharacters(in: .whitespaces) }
      .filter { !$0.isEmpty && seen.insert($0).inserted }
  }

  static let flakyExplanation = String(
    localized:
      "It sometimes fails for reasons outside the app. The mark is saved in the test and shown in the sidebar and on the project map; to keep its failures from failing Run All, put it in quarantine."
  )

  /// Why a test is shown as flaky: what its runs say, its mark in the test details, or both;
  /// `nil` when neither says so.
  static func flakyHelp(_ evidence: FlakyEvidence?, marked: Bool) -> String? {
    let parts = [evidence.map(RunHistory.flakyEvidence), marked ? String(localized: "Marked flaky in the test details.") : nil]
      .compactMap(\.self)
    return parts.isEmpty ? nil : parts.joined(separator: " ")
  }

  static func title(_ type: TestType) -> String {
    switch type {
    case .smoke: String(localized: "Smoke")
    case .regression: String(localized: "Regression")
    case .acceptance: String(localized: "Acceptance")
    }
  }

  static func title(_ priority: Priority) -> String {
    switch priority {
    case .critical: String(localized: "Critical")
    case .high: String(localized: "High")
    case .medium: String(localized: "Medium")
    case .low: String(localized: "Low")
    }
  }

  static func title(_ status: TestStatus) -> String {
    switch status {
    case .draft: String(localized: "Draft")
    case .ready: String(localized: "Ready")
    case .outdated: String(localized: "Outdated")
    }
  }
}

struct SecretName: Identifiable {
  let id: String
}

/// Asks for a secret's value and puts it into the Keychain. The value is never shown.
struct SetSecretSheet: View {
  let name: String
  var title: Text?
  var message: Text?
  var service = SecretStore.tests
  let saved: () -> Void
  @Environment(\.dismiss) private var dismiss
  @State private var value = ""
  @State private var error: String?

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      (title ?? Text("Set \(name)"))
        .font(.headline)
      (message ?? Text("The value is kept in the Keychain on this Mac. JEVision types it during runs and never shows it."))
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      SecureField(text: $value, prompt: Text("Value")) { Text("Value") }
      if let error {
        Text(error).foregroundStyle(.red).font(.callout)
      }
      HStack {
        Spacer()
        Button("Cancel", role: .cancel) { dismiss() }
          .keyboardShortcut(.cancelAction)
        Button("Save") {
          do {
            try SecretStore.save(value, for: name, in: service)
            value = ""
            saved()
            dismiss()
          } catch {
            self.error = error.localizedDescription
          }
        }
        .keyboardShortcut(.defaultAction)
        .disabled(value.isEmpty)
      }
    }
    .padding(20)
    .frame(width: 360)
  }
}

extension SetSecretSheet {
  /// Asks for the secret's value in `environment`, or for every environment when there's none.
  init(name: String, environment: String?, saved: @escaping () -> Void) {
    self.init(
      name: name,
      message: environment.map {
        Text("The value is kept in the Keychain on this Mac for \($0) runs. JEVision types it during runs and never shows it.")
      },
      service: SecretStore.service(for: environment), saved: saved)
  }
}
