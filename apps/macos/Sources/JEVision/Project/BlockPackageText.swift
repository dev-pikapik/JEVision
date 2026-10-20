import Foundation
import JEVisionCore

/// Problems with a shared block, in words. The catalog keys are the core's issue codes.
enum BlockPackageText {
  /// The sentence for `issue`, or `nil` when it isn't about a block package.
  static func message(_ issue: Issue) -> String? {
    let p = { (name: String) in issue.param(name) ?? "" }
    switch issue.code {
    case "flow.block.notInstalled":
      let block = BlockNeed(issue)?.label ?? p("id")
      return String(localized: "flow.block.notInstalled", defaultValue: "This step needs the block “\(block)”, which isn’t installed in this project. Install it, or remove the step.", bundle: .app, locale: .app)
    case "block.json.syntax":
      return String(localized: "block.json.syntax", defaultValue: "block.json isn’t valid JSON. Check line \(p("line")), column \(p("column")).", bundle: .app, locale: .app)
    case "block.json.notObject":
      return String(localized: "block.json.notObject", defaultValue: "block.json doesn’t describe a block.", bundle: .app, locale: .app)
    case "block.format.unsupported":
      return String(localized: "block.format.unsupported", defaultValue: "block.json uses format \(p("found")), and this version reads format \(p("supported")).", bundle: .app, locale: .app)
    case "block.field.missing":
      return String(localized: "block.field.missing", defaultValue: "“\(p("field"))” is required, but missing.", bundle: .app, locale: .app)
    case "block.field.empty":
      return String(localized: "block.field.empty", defaultValue: "“\(p("field"))” can’t be empty.", bundle: .app, locale: .app)
    case "block.field.tooLong":
      return String(localized: "block.field.tooLong", defaultValue: "“\(p("field"))” is too long. The most is \(p("max")) characters.", bundle: .app, locale: .app)
    case "block.field.type":
      switch p("expected") {
      case "a list":
        return String(localized: "block.field.type.list", defaultValue: "“\(p("field"))” has the wrong kind of value. It must be a list.", bundle: .app, locale: .app)
      case "an object":
        return String(localized: "block.field.type.object", defaultValue: "“\(p("field"))” has the wrong kind of value. It must be an object.", bundle: .app, locale: .app)
      default:
        return String(localized: "block.field.type.text", defaultValue: "“\(p("field"))” has the wrong kind of value. It must be text.", bundle: .app, locale: .app)
      }
    case "block.field.unknown":
      return String(localized: "block.field.unknown", defaultValue: "“\(p("field"))” isn’t a field of block.json.", bundle: .app, locale: .app)
    case "block.id.invalid":
      return String(localized: "block.id.invalid", defaultValue: "“\(p("value"))” can’t be a block id. Write three or more parts of lowercase letters, digits and - joined by dots, like com.acme.sign-in.", bundle: .app, locale: .app)
    case "block.version.invalid":
      return String(localized: "block.version.invalid", defaultValue: "“\(p("value"))” isn’t a version. Write three numbers like 1.2.0.", bundle: .app, locale: .app)
    case "block.icon.invalid":
      return String(localized: "block.icon.invalid", defaultValue: "“\(p("value"))” isn’t the name of an SF Symbol. Write lowercase letters, digits and dots, like person.badge.key.", bundle: .app, locale: .app)
    case "block.input.invalid":
      return String(localized: "block.input.invalid", defaultValue: "“\(p("name"))” can’t be an input name. Start with a letter or _, then use letters, digits and _.", bundle: .app, locale: .app)
    case "block.input.duplicate":
      return String(localized: "block.input.duplicate", defaultValue: "The input “\(p("name"))” is listed twice.", bundle: .app, locale: .app)
    case "block.input.undeclared":
      return String(localized: "block.input.undeclared", defaultValue: "The steps read the input “\(p("name"))”, but block.json doesn’t list it under inputs.", bundle: .app, locale: .app)
    case "block.input.unused":
      return String(localized: "block.input.unused", defaultValue: "block.json lists the input “\(p("name"))”, but no step reads it.", bundle: .app, locale: .app)
    case "block.version.needsNewer":
      return String(localized: "block.version.needsNewer", defaultValue: "This block needs JEVision \(p("needs")) or newer, and this is \(p("have")). Update JEVision to use it.", bundle: .app, locale: .app)
    case "block.path.outside":
      return String(localized: "block.path.outside", defaultValue: "“\(p("path"))” leads out of the block’s folder. A block can only use files inside it.", bundle: .app, locale: .app)
    case "block.file.missing":
      return String(localized: "block.file.missing", defaultValue: "The block has no \(p("file")).", bundle: .app, locale: .app)
    case "block.file.unreadable":
      return String(localized: "block.file.unreadable", defaultValue: "\(p("file")) can’t be read: \(p("reason")).", bundle: .app, locale: .app)
    case "block.folder.notPackage":
      return String(localized: "block.folder.notPackage", defaultValue: "“\(p("path"))” isn’t a block: it has no block.json and flow.json.", bundle: .app, locale: .app)
    case "block.package.tooLarge":
      return String(localized: "block.package.tooLarge", defaultValue: "The block is larger than \(size(p("max"))).", bundle: .app, locale: .app)
    case "block.package.tooManyFiles":
      return String(localized: "block.package.tooManyFiles", defaultValue: "The block has more than \(p("max")) files.", bundle: .app, locale: .app)
    case "block.path.unsafe":
      return String(localized: "block.path.unsafe", defaultValue: "“\(p("path"))” would end up outside the block’s folder, so the archive was refused.", bundle: .app, locale: .app)
    case "block.install.exists":
      return String(localized: "block.install.exists", defaultValue: "\(p("id")) \(p("version")) is already installed.", bundle: .app, locale: .app)
    case "block.install.failed":
      return String(localized: "block.install.failed", defaultValue: "Couldn’t install \(p("id")): \(p("reason")).", bundle: .app, locale: .app)
    case "block.remove.failed":
      return String(localized: "block.remove.failed", defaultValue: "Couldn’t remove \(p("id")): \(p("reason")).", bundle: .app, locale: .app)
    case "block.notInstalled":
      return String(localized: "block.notInstalled", defaultValue: "The block “\(p("id"))” isn’t installed.", bundle: .app, locale: .app)
    case "block.source.missing":
      return String(localized: "block.source.missing", defaultValue: "“\(p("source"))” doesn’t exist.", bundle: .app, locale: .app)
    case "block.source.unsupported":
      return String(localized: "block.source.unsupported", defaultValue: "“\(p("source"))” isn’t a block folder, a .zip file or an https address. GitHub repositories and releases work too.", bundle: .app, locale: .app)
    case "block.source.none":
      return String(localized: "block.source.none", defaultValue: "“\(p("id"))” wasn’t installed from an address, so there’s nothing to update it from.", bundle: .app, locale: .app)
    case "block.update.otherId":
      return String(localized: "block.update.otherId", defaultValue: "The address of “\(p("id"))” now holds a different block, “\(p("found"))”, so it isn’t an update.", bundle: .app, locale: .app)
    case "block.download.notHttps":
      return String(localized: "block.download.notHttps", defaultValue: "Only https addresses can be downloaded, and “\(p("url"))” isn’t one.", bundle: .app, locale: .app)
    case "block.download.failed":
      return String(localized: "block.download.failed", defaultValue: "Couldn’t download \(p("url")): \(p("reason")).", bundle: .app, locale: .app)
    case "block.download.tooLarge":
      return String(localized: "block.download.tooLarge", defaultValue: "\(p("url")) is larger than \(size(p("max"))).", bundle: .app, locale: .app)
    case "block.archive.invalid":
      return String(localized: "block.archive.invalid", defaultValue: "The archive can’t be read: \(p("reason")).", bundle: .app, locale: .app)
    case "block.archive.empty":
      return String(localized: "block.archive.empty", defaultValue: "The archive has no block: no folder with block.json and flow.json.", bundle: .app, locale: .app)
    case "block.archive.several":
      return String(localized: "block.archive.several", defaultValue: "The archive has several blocks (\(p("ids"))). Install them one by one.", bundle: .app, locale: .app)
    case "block.lock.invalid":
      return String(localized: "block.lock.invalid", defaultValue: "jevision.blocks.json can’t be read: \(p("reason")).", bundle: .app, locale: .app)
    case "block.export.nested":
      return String(localized: "block.export.nested", defaultValue: "This block runs “\(p("test"))”, and a block can’t hold other blocks yet. Put the steps of that block into this one.", bundle: .app, locale: .app)
    case "block.export.failed":
      return String(localized: "block.export.failed", defaultValue: "Couldn’t write the block: \(p("reason")).", bundle: .app, locale: .app)
    case "block.export.notBlock":
      return String(localized: "block.export.notBlock", defaultValue: "“\(p("path"))” isn’t one of this project’s own blocks.", bundle: .app, locale: .app)
    case "block.export.skipped":
      return String(localized: "block.export.skipped", defaultValue: "“\(p("path"))” isn’t a file a block can hold, so it was left out.", bundle: .app, locale: .app)
    default:
      return nil
    }
  }

  /// All the problems of `issues` as one text, one sentence after the other.
  static func message(_ error: BlockPackageError) -> String {
    switch error {
    case .Failed(let issues): issues.map { IssueText.message($0) }.joined(separator: " ")
    }
  }

  /// A size in bytes, written the way the Mac writes file sizes.
  static func size(_ bytes: Int64) -> String {
    ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
  }

  private static func size(_ bytes: String) -> String {
    Int64(bytes).map(size) ?? bytes
  }
}
