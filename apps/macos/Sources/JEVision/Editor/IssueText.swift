import Foundation
import JEVisionCore

/// Problems found by the core, in words. The catalog keys are the core's issue codes.
enum IssueText {
  static func message(_ issue: Issue) -> String {
    let p = { (name: String) in issue.param(name) ?? "" }
    switch issue.code {
    case "flow.json.syntax":
      return String(localized: "flow.json.syntax", defaultValue: "The file isn’t valid JSON. Check line \(p("line")), column \(p("column")).")
    case "flow.json.notObject":
      return String(localized: "flow.json.notObject", defaultValue: "The file doesn’t hold a test.")
    case "flow.format.missing":
      return String(localized: "flow.format.missing", defaultValue: "The file doesn’t say which format version it uses.")
    case "flow.format.tooNew":
      return String(localized: "flow.format.tooNew", defaultValue: "The file uses format \(p("found")) from a newer JEVision, and this version reads up to format \(p("supported")). Update the app to open it.")
    case "flow.format.unsupported":
      return String(localized: "flow.format.unsupported", defaultValue: "Format \(p("found")) is too old: this version reads format \(p("oldest")) and newer.")
    case "flow.block.unknownType":
      return String(localized: "flow.block.unknownType", defaultValue: "Block “\(p("block"))” has an unknown type “\(p("type"))”.")
    case "flow.field.missing":
      return String(localized: "flow.field.missing", defaultValue: "“\(p("field"))” is missing.")
    case "flow.field.type":
      return String(localized: "flow.field.type", defaultValue: "“\(p("field"))” has the wrong kind of value.")
    case "flow.field.value":
      return String(localized: "flow.field.value", defaultValue: "“\(p("value"))” isn’t allowed here. Use one of: \(p("allowed")).")
    case "flow.field.duration":
      return String(localized: "flow.field.duration", defaultValue: "“\(p("value"))” isn’t a duration. Write it like 500ms, 10s or 2m.")
    case "flow.field.oneOf":
      return String(localized: "flow.field.oneOf", defaultValue: "Use exactly one of: \(p("options")).")
    case "flow.field.unknown":
      return String(localized: "flow.field.unknown", defaultValue: "“\(p("field"))” isn’t a known setting.")
    case "flow.reference.thresholdNeedsSimilar":
      return String(localized: "flow.reference.thresholdNeedsSimilar", defaultValue: "A similarity threshold works only with “Similar” comparison.")
    case "flow.field.empty":
      return String(localized: "flow.field.empty", defaultValue: "This can’t be empty.")
    case "flow.field.range":
      return String(localized: "flow.field.range", defaultValue: "Must be from \(p("min")) to \(p("max")).")
    case "flow.app.invalid":
      return String(localized: "flow.app.invalid", defaultValue: "“\(p("value"))” isn’t an app identifier like com.apple.TextEdit. Choose the app again.")
    case "flow.tags.duplicate":
      return String(localized: "flow.tags.duplicate", defaultValue: "The tag “\(p("tag"))” is listed twice.")
    case "flow.block.duplicateId":
      return String(localized: "flow.block.duplicateId", defaultValue: "Two blocks are named “\(p("block"))”.")
    case "flow.block.invalidId":
      return String(localized: "flow.block.invalidId", defaultValue: "“\(p("block"))” can’t be a block name: use only letters, digits, - and _.")
    case "flow.start.unknownBlock" where p("target").isEmpty:
      return String(localized: "The test has no blocks yet.")
    case "flow.start.unknownBlock":
      return String(localized: "flow.start.unknownBlock", defaultValue: "The test starts at “\(p("target"))”, but there’s no such block.")
    case "flow.edge.unknownTarget":
      return String(localized: "flow.edge.unknownTarget", defaultValue: "Leads to “\(p("target"))”, but there’s no such block.")
    case "flow.path.invalid":
      return String(localized: "flow.path.invalid", defaultValue: "“\(p("value"))” isn’t a path inside the project. Choose the file again.")
    case "flow.path.extension":
      return String(localized: "flow.path.extension", defaultValue: "Choose a \(p("expected")) file.")
    case "flow.placeholder.invalid":
      return String(localized: "flow.placeholder.invalid", defaultValue: "“\(p("placeholder"))” isn’t a valid placeholder. Use ${secret.NAME} or ${data.column}.")
    case "flow.keys.invalid":
      return String(localized: "flow.keys.invalid", defaultValue: "“\(p("value"))” isn’t a key combination. Record the keys again.")
    case "flow.point.invalid":
      return String(localized: "flow.point.invalid", defaultValue: "The point must be inside the window.")
    case "flow.area.invalid":
      return String(localized: "flow.area.invalid", defaultValue: "The area must lie inside the image.")
    case "flow.loop.emptyBody":
      return String(localized: "flow.loop.emptyBody", defaultValue: "Nothing inside yet. Add a block to run in the loop.")
    case "flow.cycle":
      return String(localized: "flow.cycle", defaultValue: "These blocks lead round in a circle: \(p("blocks")). Use Repeat to run steps again.")
    case "flow.block.unreachable":
      return String(localized: "flow.block.unreachable", defaultValue: "Nothing leads to this block, so it never runs.")
    case "flow.loop.noReturn":
      return String(localized: "flow.loop.noReturn", defaultValue: "The blocks inside don’t lead back to this loop.")
    default:
      return String(localized: "Unknown problem: \(issue.code)")
    }
  }

  /// Name of the setting a problem is about, from the first part of its `field`, e.g. `until.appears.image`.
  static func fieldLabel(_ field: String) -> String? {
    switch field.split(separator: ".").first.map(String.init) {
    case "name": String(localized: "Name")
    case "app": String(localized: "App")
    case "text": String(localized: "Text")
    case "keys": String(localized: "Keys")
    case "files": String(localized: "Files")
    case "target": String(localized: "Target")
    case "into": String(localized: "Field")
    case "until", "reference": String(localized: "Reference")
    case "timeout": String(localized: "Time Limit")
    case "times": String(localized: "Times")
    case "file": String(localized: "Data File")
    case "test": String(localized: "Test")
    case "retries": String(localized: "Retries")
    default: nil
    }
  }

  /// The message with the name of its setting in front, for lists outside the inspector.
  static func line(_ issue: Issue) -> String {
    guard let label = issue.param("field").flatMap(fieldLabel) else { return message(issue) }
    return String(localized: "\(label): \(message(issue))")
  }
}
