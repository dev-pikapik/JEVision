import AppKit
import JEVisionCore
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

  init(defaults: UserDefaults = .standard) {
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

  func select(_ name: String) {
    guard let project else { return }
    var saved = defaults.dictionary(forKey: Self.selectionKey) as? [String: String] ?? [:]
    saved[project.path] = name
    defaults.set(saved, forKey: Self.selectionKey)
    reload()
  }

  /// Opens the environments file, first writing an example one if the project has none.
  func edit() throws {
    guard let file else { return }
    if !FileManager.default.fileExists(atPath: file.path) {
      try Self.example.write(to: file, atomically: true, encoding: .utf8)
      reload()
    }
    NSWorkspace.shared.open(file)
  }

  /// Names of every value the environments have, for inserting into a test.
  var valueNames: [String] {
    Set(current.all.flatMap(\.values.keys)).sorted()
  }

  static func file(in project: URL) -> URL {
    project.appending(path: environmentsFileName())
  }

  /// The environments of `project` and the one chosen for it on this Mac: the saved choice, or
  /// else the file's default, or else the first one.
  static func read(_ project: URL, defaults: UserDefaults = .standard) -> Snapshot {
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

  static let example = """
    {
      "format": 1,
      "environments": {
        "production": {
          "BASE_URL": "https://example.com"
        },
        "staging": {
          "BASE_URL": "https://staging.example.com"
        }
      }
    }

    """
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
