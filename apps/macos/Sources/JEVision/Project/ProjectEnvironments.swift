import AppKit
import JEVisionCore
import JEVisionHands
import Observation

/// The environments of the open project, such as Staging and Production, and the one runs use.
/// The list comes from the project's environments file; the choice is kept on this Mac, per
/// project.
@MainActor
@Observable
final class ProjectEnvironments {
  struct Snapshot {
    var all: [ProjectEnvironment] = []
    var selected: ProjectEnvironment?
    /// Why the environments file can't be read.
    var problem: String?
  }

  private(set) var current = Snapshot()
  @ObservationIgnored private var project: URL?
  @ObservationIgnored private let defaults: UserDefaults

  static let selectionKey = "selectedEnvironments"

  init(defaults: UserDefaults = Profile.current.defaults) {
    self.defaults = defaults
  }

  var file: URL? { project.map { Self.file(in: $0) } }

  func open(_ url: URL) {
    project = url
    reload()
  }

  /// Reads the environments file again, after it changed.
  func reload() {
    guard let project else { return }
    current = Self.read(project, defaults: defaults)
  }

  /// Makes `name` the environment runs use; `nil` goes back to the file's default. Undo brings
  /// back the choice made before.
  func select(_ name: String?, undoManager: UndoManager? = nil) {
    guard let project, name == nil || name != current.selected?.name else { return }
    var saved = defaults.dictionary(forKey: Self.selectionKey) as? [String: String] ?? [:]
    let before = saved[project.path]
    guard before != name else { return }
    saved[project.path] = name
    AppPreferences.set(saved, forKey: Self.selectionKey, in: defaults)
    reload()
    undoManager?.registerUndo(withTarget: self) { [weak undoManager] environments in
      MainActor.assumeIsolated { environments.select(before, undoManager: undoManager) }
    }
    undoManager?.setActionName(String(localized: "Choose Environment"))
  }

  /// Opens the environments file in the app that edits JSON.
  func edit() {
    guard let file else { return }
    NSWorkspace.shared.open(file)
  }

  /// What keeps an environment `name` with values named `valueNames` from joining the project's.
  func problem(adding name: String, valueNames: [String]) -> NewEnvironmentError? {
    guard let project else { return nil }
    return newEnvironmentProblem(project: project.path(percentEncoded: false), name: name, valueNames: valueNames)
  }

  /// Adds the environment `name` with `values` to the project's environments file, as one step
  /// for Undo in `undoManager`.
  func add(
    _ name: String, values: [String: String], undoManager: UndoManager? = nil,
    ask: @escaping FileUndo.Ask = FileUndo.askToReplace
  ) throws {
    guard let project else { return }
    try change(String(localized: "Add Environment"), in: undoManager, ask: ask) {
      try addEnvironment(project: project.path(percentEncoded: false), name: name, values: values)
    }
  }

  /// Sets the value `name` of `environment` in the project's environments file, adding it when
  /// the environment lacks it, as one step for Undo in `undoManager`.
  func setValue(
    _ value: String, named name: String, in environment: String, undoManager: UndoManager? = nil,
    ask: @escaping FileUndo.Ask = FileUndo.askToReplace
  ) throws {
    guard let project else { return }
    try change(String(localized: "Change Value"), in: undoManager, ask: ask) {
      try setEnvironmentValue(project: project.path(percentEncoded: false), environment: environment, name: name, value: value)
    }
  }

  /// The value `name` of `environment` as the file holds it now, read again rather than as last
  /// shown; `nil` when it isn't set.
  func value(_ name: String, in environment: String) -> String? {
    reload()
    return current.all.first { $0.name == environment }?.values[name]
  }

  /// Runs `write`, which changes the environments file, and shows the file after it and after
  /// Undo or Redo of it.
  private func change(
    _ action: String, in undoManager: UndoManager?, ask: @escaping FileUndo.Ask, _ write: () throws -> Void
  ) throws {
    guard let file else { return }
    try FileUndo.record(file, action: action, in: undoManager, ask: ask, then: { [weak self] in self?.reload() }, write)
    reload()
  }

  /// Why a value couldn't be set, in words for the person.
  static func describe(_ error: Error) -> String {
    switch error {
    case EnvironmentValueError.UnknownEnvironment(let name):
      String(localized: "There’s no environment \(name).")
    case EnvironmentValueError.InvalidValueName(let name):
      String(localized: "“\(name)” can’t be a value name: use Latin letters, digits and _, not starting with a digit.")
    case EnvironmentValueError.Unreadable(let file, let reason):
      String(localized: "\(file) can’t be read: \(reason)")
    case EnvironmentValueError.Unwritable(let file, let reason):
      String(localized: "\(file) can’t be saved: \(reason)")
    default:
      error.localizedDescription
    }
  }

  /// Names of every value the environments have, and the one every run has, for inserting into
  /// a test.
  var valueNames: [String] {
    Set(current.all.flatMap(\.values.keys)).union([environmentNameValue()]).sorted()
  }

  static func file(in project: URL) -> URL {
    project.appending(path: environmentsFileName())
  }

  /// The environments of `project` and the one chosen for it on this Mac: the saved choice, or
  /// else the file's default, or else the first one.
  static func read(_ project: URL, defaults: UserDefaults = Profile.current.defaults) -> Snapshot {
    do {
      let path = project.path(percentEncoded: false)
      let all = try projectEnvironments(project: path)
      let fallback = try defaultEnvironment(project: path)
      let saved = (defaults.dictionary(forKey: selectionKey) as? [String: String])?[project.path]
      let named = { (name: String?) in all.first { $0.name == name } }
      return Snapshot(all: all, selected: named(saved) ?? named(fallback) ?? all.first)
    } catch EnvironmentsError.Unreadable(let file, let reason) {
      return Snapshot(problem: String(localized: "\(file) can’t be read: \(reason)"))
    } catch {
      return Snapshot(problem: error.localizedDescription)
    }
  }
}

/// What keeps a run in the chosen environment from finding a value the test reads. Values are
/// never part of it, only names.
struct MissingValues: Equatable {
  /// The environment the run would use.
  var environment: String?
  /// `${env.NAME}` values that environment lacks; all of them when there's no environment.
  var values: [String] = []
  /// Secrets set neither for that environment nor for all.
  var secrets: [String] = []
  /// The environments file can't be read.
  var problem: String?

  var isEmpty: Bool { values.isEmpty && secrets.isEmpty && problem == nil }

  init(environment: String? = nil, values: [String] = [], secrets: [String] = [], problem: String? = nil) {
    self.environment = environment
    self.values = values
    self.secrets = secrets
    self.problem = problem
  }

  /// What `flow` needs that `environments` and the secrets don't give it.
  init(for flow: Flow, in environments: ProjectEnvironments.Snapshot, isSet: (String, String?) -> Bool) {
    let selected = environments.selected
    environment = selected?.name
    let needed = requiredEnvValues(flow: flow)
    problem = needed.isEmpty ? nil : environments.problem
    values = needed.filter { selected?.values[$0] == nil }
    secrets = requiredSecrets(flow: flow).filter { !isSet($0, selected?.name) }
  }
}

extension ProjectEnvironments.Snapshot {
  /// What a block that reads `names` finds in the chosen environment, a line for each, for its
  /// tooltip; `nil` when it reads none.
  func help(reading names: [String]) -> String? {
    guard !names.isEmpty else { return nil }
    let builtin = environmentNameValue()
    return names.map { name in
      let own = selected?.values[name]
      if let own { return String(localized: "\(name) = \(own)") }
      guard let selected else { return String(localized: "\(name): no environment is chosen") }
      if name == builtin { return String(localized: "\(name) = \(selected.name)") }
      return String(localized: "\(name) isn’t set in \(selected.name)")
    }
    .joined(separator: "\n")
  }
}

/// What the canvas shows of the environment a run of the test would use: its name and the
/// values the test reads from it. Secrets aren't here; the canvas shows only whether they're set.
struct EnvironmentReadout: Equatable {
  struct Value: Equatable {
    let name: String
    /// `nil` when the environment has no such value.
    let value: String?
  }

  /// The environment the run would use.
  var environment: String?
  var values: [Value]

  init(for flow: Flow, in snapshot: ProjectEnvironments.Snapshot) {
    environment = snapshot.selected?.name
    values = requiredEnvValues(flow: flow).map { Value(name: $0, value: snapshot.selected?.values[$0]) }
  }

  var missing: [String] { values.filter { $0.value == nil }.map(\.name) }

  /// The values on one line, such as `HOST s.example.com, USER`; the first `limit` of them, then
  /// how many more there are.
  func summary(limit: Int = 2) -> String? {
    guard !values.isEmpty else { return nil }
    let shown = values.prefix(limit).map { value in value.value.map { "\(value.name) \($0)" } ?? value.name }
    let more = values.count - shown.count
    return more > 0 ? (shown + [String(localized: "+\(more)")]).joined(separator: ", ") : shown.joined(separator: ", ")
  }
}
