import Foundation
import JEVisionCore
import Observation

/// What the New Project sheet asks for and does: a new project the core makes, empty or a copy
/// of the example or of another project, or a copy of a repository from a server.
@MainActor
@Observable
final class NewProjectModel {
  enum Mode: Hashable {
    case new, clone
  }

  enum Starter: Hashable {
    case empty, example, copy
  }

  /// A field that says what went wrong under it.
  enum Field {
    case name, address
  }

  struct Failure: Equatable {
    let field: Field
    let message: String
    /// Git's own line about it, for those who want to know more; empty when there is none.
    var detail = ""
  }

  var mode: Mode {
    didSet {
      failure = nil
      suggestName()
    }
  }
  var name = "" {
    didSet {
      if name != oldValue { failure = nil }
    }
  }
  /// The folder the project goes in.
  var location: URL {
    didSet {
      failure = nil
      suggestName()
    }
  }
  /// The folders Where offers: those the core suggests, then any chosen with Other….
  private(set) var locations: [URL]
  private(set) var starter: Starter
  /// The project a new one is a copy of, once chosen.
  private(set) var copySource: URL?
  /// The address of the repository to copy.
  var address = "" {
    didSet {
      guard address != oldValue else { return }
      failure = nil
      suggestName()
    }
  }
  /// Where the history of changes of a new project in `location` is kept; `nil` until looked at.
  private(set) var history: NewProjectHistory?
  /// A project is being made or a repository copied.
  private(set) var isWorking = false
  private(set) var failure: Failure?

  /// The bundled example; `nil` when the app runs without one.
  let example: URL?
  @ObservationIgnored private let home: URL
  /// The name last filled in for the person, replaced by a better one until they type their own.
  @ObservationIgnored private var suggested = ""
  @ObservationIgnored private var cloning: RepositoryClone?
  /// Cancel was pressed while the repository was being copied.
  @ObservationIgnored private var cloneCancelled = false

  /// `copied` is text the person copied: a repository address in it is offered for a copy.
  init(_ request: NewProjectRequest, home: URL = .homeDirectory, example: URL? = ExampleProject.bundled(), copied: String? = nil) {
    self.home = home
    self.example = example
    let locations = projectLocations(home: home.path(percentEncoded: false)).map { URL(filePath: $0, directoryHint: .isDirectory) }
    self.locations = locations
    location = locations[0]
    mode = request == .clone ? .clone : .new
    starter = request == .example && example != nil ? .example : .empty
    if request == .clone, let copied { address = repositoryAddress(text: copied) ?? "" }
    suggestName()
  }

  // MARK: Choices

  /// Picks Empty or With Example; Copy of Project goes through `copy(of:)`.
  func choose(_ starter: Starter) {
    guard starter != .copy, starter != .example || example != nil else { return }
    self.starter = starter
    suggestName()
  }

  /// Makes the new project a copy of the project in `folder`.
  func copy(of folder: URL) {
    copySource = folder
    starter = .copy
    suggestName()
  }

  /// Takes the address from text the person pasted, trimmed.
  func paste(_ text: String) {
    address = repositoryAddress(text: text) ?? text.trimmingCharacters(in: .whitespacesAndNewlines)
  }

  /// Puts the project in `folder`, chosen with Other…, and offers it under Where from now on.
  func choose(location folder: URL) {
    if !locations.contains(folder) { locations.append(folder) }
    location = folder
  }

  /// How Where names `folder`: “Documents › JEVision” for a folder in Documents, else its path
  /// with `~`.
  func title(of folder: URL) -> String {
    let documents = home.appending(path: "Documents", directoryHint: .isDirectory)
    guard folder.deletingLastPathComponent().standardizedFileURL.path == documents.standardizedFileURL.path else {
      return ProjectStore.shortPath(folder.path)
    }
    return String(localized: "Documents › \(folder.lastPathComponent)")
  }

  /// The folder the project will be, as people read it; `nil` before there is a name.
  var target: String? {
    let name = name.trimmingCharacters(in: .whitespaces)
    guard !name.isEmpty else { return nil }
    return ProjectStore.shortPath(location.appending(path: name).path)
  }

  /// Why the name won't do in `location`, as the core says; `nil` when it will.
  var nameProblem: String? {
    projectNameProblem(parent: location.path(percentEncoded: false), name: name)
  }

  /// What the starter is in the core's words; `nil` when the folder to copy isn't there to copy.
  var coreStarter: ProjectStarter? {
    switch starter {
    case .empty: .empty
    case .example: example.map { .copyOf(folder: $0.path(percentEncoded: false)) }
    case .copy: copySource.map { .copyOf(folder: $0.path(percentEncoded: false)) }
    }
  }

  var canCreate: Bool {
    !isWorking && nameProblem == nil
      && (mode == .clone ? !address.trimmingCharacters(in: .whitespaces).isEmpty : coreStarter != nil)
  }

  /// Fills in a name from the starter or the address while the person hasn't typed one of their own.
  private func suggestName() {
    guard name.trimmingCharacters(in: .whitespaces).isEmpty || name == suggested else { return }
    let base: String =
      switch mode {
      case .clone: cloneFolderName(url: address)
      case .new where starter == .example: ExampleProject.name
      case .new where starter == .copy: copySource?.lastPathComponent ?? ""
      case .new: ""
      }
    // A copy takes the next free name; a repository keeps its own, so a taken one is said.
    let free =
      base.isEmpty || mode == .clone
      ? base
      : RelativePath.unique(base) { projectNameProblem(parent: location.path(percentEncoded: false), name: $0) != nil }
    suggested = free
    name = free
  }

  /// Looks at where the history of a project in `location` would be kept, off the main thread:
  /// git is asked.
  func lookAtHistory() async {
    let parent = location.path(percentEncoded: false)
    let found = await Task.detached(priority: .userInitiated) { newProjectHistory(parent: parent) }.value
    if parent == location.path(percentEncoded: false) { history = found }
  }

  // MARK: Making

  /// Makes the project; `nil`, with the reason under the name, when it couldn't be made.
  func create() async -> CreatedProject? {
    guard canCreate, let starter = coreStarter else { return nil }
    isWorking = true
    defer { isWorking = false }
    let (parent, name) = (location.path(percentEncoded: false), name)
    do {
      return try await Task.detached(priority: .userInitiated) {
        try createProject(parent: parent, name: name, starter: starter)
      }.value
    } catch {
      failure = Failure(field: .name, message: ProjectStore.message(error))
      return nil
    }
  }

  /// The test to show first in `project`: the example's own first test, else the core's choice.
  func firstTest(of project: CreatedProject) -> String? {
    starter == .example ? ExampleProject.firstTest : project.firstTest
  }

  /// Copies the repository; its folder, or `nil` when it was cancelled or failed, with the reason
  /// under the field it is about.
  func clone() async -> URL? {
    guard canCreate else { return nil }
    let job = RepositoryClone()
    cloning = job
    cloneCancelled = false
    isWorking = true
    defer {
      cloning = nil
      isWorking = false
    }
    let (url, parent, name) = (address, location.path(percentEncoded: false), name)
    do {
      let path = try await Task.detached(priority: .userInitiated) { try job.run(url: url, parent: parent, name: name) }.value
      let folder = URL(filePath: path, directoryHint: .isDirectory)
      // Cancel was pressed just as git finished: the copy is undone, as one stopped midway is.
      guard !cloneCancelled else {
        try? FileManager.default.removeItem(at: folder)
        return nil
      }
      return folder
    } catch CloneError.Cancelled {
      return nil
    } catch CloneError.Failed(let problem, let message, let detail) {
      let field: Field = [.folderExists, .invalidName].contains(problem) && !url.trimmingCharacters(in: .whitespaces).isEmpty ? .name : .address
      failure = Failure(field: field, message: message, detail: detail)
      return nil
    } catch {
      failure = Failure(field: .address, message: error.localizedDescription)
      return nil
    }
  }

  /// Stops the copy under way: git stops and nothing it made is left, even when it was just
  /// finishing.
  func cancelClone() {
    guard let cloning else { return }
    cloneCancelled = true
    cloning.cancel()
  }
}
