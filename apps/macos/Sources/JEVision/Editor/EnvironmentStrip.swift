import JEVisionCore
import JEVisionHands
import SwiftUI

/// The environment a run of the test uses, on its canvas: the name and the values the test reads
/// from it. A click opens a panel to pick another environment, change those values and see
/// which secrets are set.
struct EnvironmentStrip: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.undoManager) private var undoManager
  let flow: Flow
  @State private var open = false

  var body: some View {
    let snapshot = project.environments.current
    let readout = EnvironmentReadout(for: flow, in: snapshot)
    let unset = requiredSecrets(flow: flow).filter { !SecretStore.isSet($0, environment: readout.environment) }
    let needsAttention = snapshot.problem != nil || !readout.missing.isEmpty || !unset.isEmpty
    if !snapshot.all.isEmpty || snapshot.problem != nil || !readout.values.isEmpty || needsAttention {
      Button { open.toggle() } label: {
        HStack(spacing: 6) {
          Image(systemName: needsAttention ? "exclamationmark.triangle.fill" : "server.rack")
            .foregroundStyle(needsAttention ? Color.orange : Color.secondary)
          Text(verbatim: readout.environment ?? String(localized: "No Environment"))
            .font(.callout.weight(.medium))
          if let summary = readout.summary() {
            Text(verbatim: summary)
              .font(.callout)
              .foregroundStyle(.secondary)
              .lineLimit(1)
              .truncationMode(.tail)
          }
          Image(systemName: "chevron.down").imageScale(.small).foregroundStyle(.secondary)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .frame(maxWidth: 400)
        .adaptiveGlass()
        .contentShape(.capsule)
      }
      .buttonStyle(.plain)
      .help(Text("The environment this test runs in. Click to change it or its values."))
      .accessibilityLabel(Text("Environment"))
      .accessibilityValue(Text(verbatim: spoken(readout, snapshot: snapshot, unset: unset)))
      .accessibilityHint(Text("Opens the environment’s values and the secrets this test needs."))
      .popover(isPresented: $open, arrowEdge: .bottom) { EnvironmentPanel(flow: flow, undoManager: undoManager) }
    }
  }

  /// What VoiceOver reads: the environment, what the test finds in it and the secrets not set.
  private func spoken(_ readout: EnvironmentReadout, snapshot: ProjectEnvironments.Snapshot, unset: [String]) -> String {
    [
      readout.environment ?? String(localized: "No Environment"),
      snapshot.help(reading: readout.values.map(\.name))?.replacingOccurrences(of: "\n", with: ". "),
      unset.isEmpty ? nil : String(localized: "Secrets not set: \(unset.joined(separator: ", "))"),
    ]
    .compactMap(\.self)
    .joined(separator: ". ")
  }
}

/// Picks the environment, edits the values the test reads from it and shows its secrets.
private struct EnvironmentPanel: View {
  @Environment(ProjectStore.self) private var project
  let flow: Flow
  /// The editor window's: the panel's changes are undone there, after the panel closes too.
  let undoManager: UndoManager?
  @State private var settingSecret: SecretName?
  @State private var adding = false
  @State private var failure: String?
  /// Bumped after a secret is saved, to read the statuses again.
  @State private var secretsVersion = 0

  var body: some View {
    let snapshot = project.environments.current
    let readout = EnvironmentReadout(for: flow, in: snapshot)
    let secrets = requiredSecrets(flow: flow)
    Form {
      Section {
        if !snapshot.all.isEmpty {
          Picker(selection: Binding(get: { snapshot.selected?.name ?? "" }, set: { project.environments.select($0, undoManager: undoManager) })) {
            ForEach(snapshot.all, id: \.name) { Text(verbatim: $0.name).tag($0.name) }
          } label: {
            Text("Environment")
          }
        }
        if let problem = snapshot.problem {
          Label {
            Text(problem)
          } icon: {
            Image(systemName: "exclamationmark.octagon.fill").foregroundStyle(.red)
          }
        }
      }
      if !readout.values.isEmpty {
        Section {
          ForEach(readout.values, id: \.name) { value in
            if let environment = readout.environment {
              LabeledContent {
                ValueField(name: value.name, text: value.value ?? "") {
                  project.environments.value(value.name, in: environment)
                } save: {
                  save($0, named: value.name, in: environment)
                }
                  .labelsHidden()
                  .accessibilityLabel(Text(verbatim: value.name))
              } label: {
                Text(verbatim: value.name)
              }
            } else {
              Label {
                Text(verbatim: value.name)
              } icon: {
                Image(systemName: "server.rack").foregroundStyle(.orange)
              }
            }
          }
          if let failure {
            Text(failure).foregroundStyle(.red).font(.callout)
          }
        } header: {
          Text("Values This Test Reads")
        } footer: {
          Group {
            if readout.environment == nil {
              Text("The project has no environment yet. Add one to give the test its values.")
            } else {
              Text("Changes are saved in \(environmentsFileName()) at once. Don’t put secrets here.")
            }
          }
          .foregroundStyle(.secondary)
        }
      }
      if !secrets.isEmpty {
        Section {
          ForEach(secrets, id: \.self) { name in
            SecretStatusRow(
              name: name,
              isSet: secretsVersion >= 0 && SecretStore.isSet(name, environment: readout.environment)
            ) { settingSecret = SecretName(id: name) }
          }
        } header: {
          if let environment = readout.environment {
            Text("Secrets in \(environment)")
          } else {
            Text("Secrets")
          }
        }
      }
      Section {
        Button("Add Environment…") { adding = true }
          .disabled(snapshot.problem != nil)
        if !snapshot.all.isEmpty || snapshot.problem != nil {
          Button("Edit Environments…") { project.environments.edit() }
        }
      }
    }
    .formStyle(.grouped)
    .frame(width: 380)
    .frame(minHeight: 160, maxHeight: 520)
    .sheet(item: $settingSecret) { secret in
      SetSecretSheet(name: secret.id, environment: readout.environment) { secretsVersion += 1 }
    }
    .sheet(isPresented: $adding) {
      AddEnvironmentSheet(valueNames: readout.values.map(\.name), undoManager: undoManager)
    }
  }

  private func save(_ value: String, named name: String, in environment: String) {
    do {
      try project.environments.setValue(value, named: name, in: environment, undoManager: undoManager)
      failure = nil
    } catch {
      failure = ProjectEnvironments.describe(error)
    }
  }
}

/// One line of text saved when the field is left or Return is pressed, not on every key. When the
/// file got another value while the person typed, they choose which one stays.
private struct ValueField: View {
  /// The value's name, for the question.
  let name: String
  let text: String
  /// The value as the file holds it now; `nil` when it isn't set.
  let read: () -> String?
  let save: (String) -> Void
  @State private var draft = ""
  /// The value the draft began from.
  @State private var base = ""
  /// The person is being asked which value stays.
  @State private var asking = false
  @FocusState private var focused: Bool

  var body: some View {
    TextField(text: $draft, prompt: Text("Not set")) { Text("Value") }
      .focused($focused)
      .multilineTextAlignment(.trailing)
      .onSubmit(commit)
      .onChange(of: focused) { if !focused { commit() } }
      .onDisappear(perform: commit)
      .onChange(of: text, initial: true) {
        if !focused || draft == base || draft == text {
          draft = text
          base = text
        }
      }
  }

  private func commit() {
    guard !asking, draft != base else { return }
    let file = read()
    switch ValueDraft(typed: draft, base: base, file: file ?? "") {
    case .unchanged:
      return
    case .save:
      save(draft)
    case .conflict:
      asking = true
      if ValueDraft.keepsTyped(draft, over: file, name: name) {
        save(draft)
      } else {
        draft = file ?? ""
      }
      base = file ?? ""
      asking = false
    }
  }
}

/// What saving a value the person typed does, when the file may have changed since they began.
enum ValueDraft: Equatable {
  /// Nothing to save: nothing new was typed, or the file holds it already.
  case unchanged
  case save
  /// The file got another value while the person typed: they choose which one stays.
  case conflict

  /// `typed` was typed over `base`, the value the person began from; `file` is what the file
  /// holds now. `nil` is a value that isn't set.
  init(typed: String, base: String?, file: String?) {
    if typed == file || typed == base {
      self = .unchanged
    } else if file == base {
      self = .save
    } else {
      self = .conflict
    }
  }

  /// Asks whether what the person typed for `name` replaces the value the file got meanwhile;
  /// true replaces it.
  @MainActor
  static func keepsTyped(_ typed: String, over file: String?, name: String) -> Bool {
    let alert = NSAlert()
    alert.alertStyle = .warning
    alert.messageText = String(localized: "\(name) was changed in the file while you typed")
    alert.informativeText =
      file.map { String(localized: "The file now has “\($0)”. You typed “\(typed)”.") }
      ?? String(localized: "It’s no longer set in the file. You typed “\(typed)”.")
    alert.addButton(withTitle: String(localized: "Keep the File’s Value"))
    alert.addButton(withTitle: String(localized: "Use What I Typed"))
    return alert.runModal() == .alertSecondButtonReturn
  }
}

/// Whether a secret is set on this Mac, with a button to set it. The value is never shown.
struct SecretStatusRow: View {
  let name: String
  let isSet: Bool
  let set: () -> Void

  var body: some View {
    LabeledContent {
      HStack {
        Label {
          Text(isSet ? "Set" : "Not set")
        } icon: {
          Image(systemName: isSet ? "checkmark.circle.fill" : "circle.dashed")
            .foregroundStyle(isSet ? .green : .secondary)
        }
        Button("Set…", action: set)
          .accessibilityLabel(Text("Set \(name)"))
      }
    } label: {
      Text(verbatim: name)
    }
  }
}

/// A value the chosen environment lacks, for the test card to ask for.
struct EnvironmentValueName: Identifiable {
  let id: String
}

/// Asks for the value of a name in an environment and saves it to the environments file.
struct SetEnvironmentValueSheet: View {
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss
  let name: String
  let environment: String
  /// The presenting window's: saving is undone there once the sheet has closed.
  let undoManager: UndoManager?
  @State private var value = ""
  /// The value the file had when the sheet opened.
  @State private var base: String?
  @State private var failure: String?

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      Text("Set \(name)")
        .font(.headline)
      Text("The value is saved for \(environment) in \(environmentsFileName()), next to the tests. Don’t put secrets here.")
        .foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
      TextField(text: $value, prompt: Text("Value")) { Text("Value") }
        .onSubmit(save)
      if let failure {
        Text(failure).foregroundStyle(.red).font(.callout)
      }
      HStack {
        Spacer()
        Button("Cancel", role: .cancel) { dismiss() }
          .keyboardShortcut(.cancelAction)
        Button("Save", action: save)
          .keyboardShortcut(.defaultAction)
      }
    }
    .padding(20)
    .frame(width: 360)
    .onAppear { base = project.environments.current.all.first { $0.name == environment }?.values[name] }
  }

  private func save() {
    let file = project.environments.value(name, in: environment)
    switch ValueDraft(typed: value, base: base, file: file) {
    case .unchanged:
      dismiss()
      return
    case .save:
      break
    case .conflict:
      guard ValueDraft.keepsTyped(value, over: file, name: name) else {
        dismiss()
        return
      }
    }
    do {
      try project.environments.setValue(value, named: name, in: environment, undoManager: undoManager)
      dismiss()
    } catch {
      failure = ProjectEnvironments.describe(error)
    }
  }
}
