import Foundation
import JEVisionCore
import Observation

/// A part of a test kept for reuse: a template, whose blocks are copied into the test, or a
/// custom block, a test other tests run as one step.
struct LibraryEntry: Hashable, Identifiable, Sendable {
  enum Kind: Hashable, Identifiable, Sendable {
    case template, block

    var id: Self { self }
  }

  let kind: Kind
  /// `builtin/NAME` for the templates every project has, otherwise the file relative to the project.
  let id: String
  let name: String
  let detail: String
  let symbol: String
  /// The text of the template; `nil` for a custom block.
  let text: String?
  /// The file; `nil` for a built-in template.
  let url: URL?
  /// What a custom block asks for each time it's used, as `${input.NAME}` names.
  var inputs: [String] = []
}

/// The templates and custom blocks of the open project, from its `templates` and `blocks`
/// folders, after the built-in templates.
@MainActor
@Observable
final class ProjectLibrary {
  nonisolated static let templatesFolder = "templates"
  nonisolated static let blocksFolder = "blocks"
  nonisolated static let templateSuffix = ".template.json"
  nonisolated static let blockSuffix = ".block.json"

  private(set) var entries: [LibraryEntry] = ProjectLibrary.builtins
  @ObservationIgnored private var project: URL?

  func open(_ url: URL) {
    project = url
    reload()
  }

  /// Reads the folders again, after something in the project changed.
  func reload() {
    let found = Self.builtins + (project.map(Self.read) ?? [])
    if found != entries { entries = found }
  }

  var templates: [LibraryEntry] { entries.filter { $0.kind == .template } }
  var blocks: [LibraryEntry] { entries.filter { $0.kind == .block } }

  /// Saves `text` into the project as a template or a custom block named after `name`, next to
  /// the others, and returns the new file.
  func save(_ text: String, name: String, as kind: LibraryEntry.Kind) throws -> URL {
    guard let project else { throw CocoaError(.fileNoSuchFile) }
    let (folderName, suffix) = Self.place(of: kind)
    let folder = project.appending(path: folderName)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let stem = ProjectScanner.testName(RelativePath.testFileName(name))
    let fileName =
      RelativePath.unique(stem) { FileManager.default.fileExists(atPath: folder.appending(path: $0 + suffix).path) }
      + suffix
    let url = folder.appending(path: fileName)
    try Data(text.utf8).write(to: url, options: .withoutOverwriting)
    reload()
    return url
  }

  private static func place(of kind: LibraryEntry.Kind) -> (folder: String, suffix: String) {
    switch kind {
    case .template: (templatesFolder, templateSuffix)
    case .block: (blocksFolder, blockSuffix)
    }
  }

  /// The templates and blocks saved in `project`, each kind by name. Files that aren't tests are left out.
  static func read(_ project: URL) -> [LibraryEntry] {
    [LibraryEntry.Kind.template, .block].flatMap { kind in
      let (folderName, suffix) = place(of: kind)
      let folder = project.appending(path: folderName)
      let files = (try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)) ?? []
      return files
        .filter { $0.lastPathComponent.lowercased().hasSuffix(suffix) }
        .compactMap { url -> LibraryEntry? in
          guard let text = try? String(contentsOf: url, encoding: .utf8), let flow = try? loadFlow(json: text) else {
            return nil
          }
          let fileName = url.lastPathComponent
          return LibraryEntry(
            kind: kind, id: folderName + "/" + fileName,
            name: flow.name.isEmpty ? String(fileName.dropLast(suffix.count)) : flow.name,
            detail: flow.description ?? "",
            symbol: kind == .block ? flow.icon ?? defaultBlockSymbol : templateSymbol,
            text: kind == .template ? text : nil, url: url, inputs: kind == .block ? requiredInputs(flow: flow) : [])
        }
        .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }
  }

  nonisolated static let templateSymbol = "square.on.square.dashed"
  nonisolated static let defaultBlockSymbol = "shippingbox"

  /// The templates every project has, named in the user's language.
  static let builtins: [LibraryEntry] = builtinTemplates().map { template in
    let (name, detail) = builtinText(template.id)
    return LibraryEntry(
      kind: .template, id: "builtin/" + template.id, name: name, detail: detail, symbol: templateSymbol,
      text: template.text, url: nil)
  }

  private static func builtinText(_ id: String) -> (String, String) {
    switch id {
    case "login":
      (
        String(localized: "Sign In"),
        String(localized: "Types an email and a password, then checks the screen after signing in.")
      )
    case "onboarding":
      (
        String(localized: "Onboarding"),
        String(localized: "Clicks Continue through the welcome screens, then checks the screen that follows.")
      )
    case "payment":
      (
        String(localized: "Payment"),
        String(localized: "Fills in the card form, pays and checks the confirmation.")
      )
    default: (id, "")
    }
  }
}
