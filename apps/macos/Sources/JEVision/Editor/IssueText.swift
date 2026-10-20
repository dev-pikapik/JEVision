import Foundation
import JEVisionCore

/// Problems found by the core, in words. The catalog keys are the core's issue codes.
enum IssueText {
  static func message(_ issue: Issue) -> String {
    let p = { (name: String) in issue.param(name) ?? "" }
    switch issue.code {
    case "flow.json.syntax":
      return String(localized: "flow.json.syntax", defaultValue: "The file isn’t valid JSON. Check line \(p("line")), column \(p("column")).", bundle: .app, locale: .app)
    case "flow.json.notObject":
      return String(localized: "flow.json.notObject", defaultValue: "The file doesn’t hold a test.", bundle: .app, locale: .app)
    case "flow.format.missing":
      return String(localized: "flow.format.missing", defaultValue: "The file doesn’t say which format version it uses.", bundle: .app, locale: .app)
    case "flow.format.tooNew":
      return String(localized: "flow.format.tooNew", defaultValue: "The file uses format \(p("found")) from a newer JEVision, and this version reads up to format \(p("supported")). Update the app to open it.", bundle: .app, locale: .app)
    case "flow.format.unsupported":
      return String(localized: "flow.format.unsupported", defaultValue: "Format \(p("found")) is too old: this version reads format \(p("oldest")) and newer.", bundle: .app, locale: .app)
    case "flow.block.unknownType":
      return String(localized: "flow.block.unknownType", defaultValue: "Block “\(p("block"))” has an unknown type “\(p("type"))”.", bundle: .app, locale: .app)
    case "flow.field.missing":
      return String(localized: "flow.field.missing", defaultValue: "“\(p("field"))” is missing.", bundle: .app, locale: .app)
    case "flow.field.type":
      return String(localized: "flow.field.type", defaultValue: "“\(p("field"))” has the wrong kind of value.", bundle: .app, locale: .app)
    case "flow.field.value":
      return String(localized: "flow.field.value", defaultValue: "“\(p("value"))” isn’t allowed here. Use one of: \(p("allowed")).", bundle: .app, locale: .app)
    case "flow.field.duration":
      return String(localized: "flow.field.duration", defaultValue: "“\(p("value"))” isn’t a duration. Write it like 500ms, 10s or 2m.", bundle: .app, locale: .app)
    case "flow.field.oneOf":
      return String(localized: "flow.field.oneOf", defaultValue: "Use exactly one of: \(p("options")).", bundle: .app, locale: .app)
    case "flow.field.unknown":
      return String(localized: "flow.field.unknown", defaultValue: "“\(p("field"))” isn’t a known setting.", bundle: .app, locale: .app)
    case "flow.reference.expectNeedsMeaning":
      return String(localized: "flow.reference.expectNeedsMeaning", defaultValue: "An expected text works only with “By Meaning” comparison.", bundle: .app, locale: .app)
    case "flow.reference.modelNeedsMeaning":
      return String(localized: "flow.reference.modelNeedsMeaning", defaultValue: "A model works only with “By Meaning” comparison.", bundle: .app, locale: .app)
    case "flow.reference.providerNeedsMeaning":
      return String(localized: "flow.reference.providerNeedsMeaning", defaultValue: "An AI choice works only with “By Meaning” comparison.", bundle: .app, locale: .app)
    case "flow.reference.unknownProvider":
      return String(
        localized: "flow.reference.unknownProvider",
        defaultValue: "“\(p("provider"))” isn’t an AI JEVision knows. Use one of: \(p("allowed")).", bundle: .app, locale: .app)
    case "flow.reference.thresholdNeedsSimilar":
      return String(localized: "flow.reference.thresholdNeedsSimilar", defaultValue: "A similarity threshold works only with “Similar” comparison.", bundle: .app, locale: .app)
    case "flow.field.empty":
      return String(localized: "flow.field.empty", defaultValue: "This can’t be empty.", bundle: .app, locale: .app)
    case "flow.field.range":
      return String(localized: "flow.field.range", defaultValue: "Must be from \(p("min")) to \(p("max")).", bundle: .app, locale: .app)
    case "flow.app.invalid":
      return String(localized: "flow.app.invalid", defaultValue: "“\(p("value"))” isn’t an app identifier like com.apple.TextEdit. Choose the app again.", bundle: .app, locale: .app)
    case "flow.link.invalid":
      return String(localized: "flow.link.invalid", defaultValue: "“\(p("value"))” isn’t a web address. Start it with https://.", bundle: .app, locale: .app)
    case "flow.tags.duplicate":
      return String(localized: "flow.tags.duplicate", defaultValue: "The tag “\(p("tag"))” is listed twice.", bundle: .app, locale: .app)
    case "flow.dependency.invalid":
      return String(localized: "flow.dependency.invalid", defaultValue: "“\(p("value"))” isn’t a test file inside the project folder. Pick the test again in Runs After.", bundle: .app, locale: .app)
    case "flow.dependency.duplicate":
      return String(localized: "flow.dependency.duplicate", defaultValue: "“\(p("test"))” is listed twice in Runs After.", bundle: .app, locale: .app)
    case "flow.dependency.missing":
      return String(localized: "flow.dependency.missing", defaultValue: "The test “\(p("test"))” in Runs After doesn’t exist. Remove it or pick another test.", bundle: .app, locale: .app)
    case "flow.dependency.self":
      return String(localized: "flow.dependency.self", defaultValue: "A test can’t run after itself. Remove it from Runs After.", bundle: .app, locale: .app)
    case "flow.dependency.cycle":
      return String(localized: "flow.dependency.cycle", defaultValue: "The tests wait for each other in a circle: \(p("cycle")). Remove one of them from Runs After.", bundle: .app, locale: .app)
    case "flow.block.duplicateId":
      return String(localized: "flow.block.duplicateId", defaultValue: "Two blocks are named “\(p("block"))”.", bundle: .app, locale: .app)
    case "flow.block.invalidId":
      return String(localized: "flow.block.invalidId", defaultValue: "“\(p("block"))” can’t be a block name: use only letters, digits, - and _.", bundle: .app, locale: .app)
    case "flow.start.unknownBlock" where p("target").isEmpty:
      return String(localized: "The test has no blocks yet.")
    case "flow.start.unknownBlock":
      return String(localized: "flow.start.unknownBlock", defaultValue: "The test starts at “\(p("target"))”, but there’s no such block.", bundle: .app, locale: .app)
    case "flow.edge.unknownTarget":
      return String(localized: "flow.edge.unknownTarget", defaultValue: "Leads to “\(p("target"))”, but there’s no such block.", bundle: .app, locale: .app)
    case "flow.path.invalid":
      return String(localized: "flow.path.invalid", defaultValue: "“\(p("value"))” isn’t a path inside the project. Choose the file again.", bundle: .app, locale: .app)
    case "flow.path.extension":
      return String(localized: "flow.path.extension", defaultValue: "Choose a \(p("expected")) file.", bundle: .app, locale: .app)
    case "flow.placeholder.invalid":
      return String(localized: "flow.placeholder.invalid", defaultValue: "“\(p("placeholder"))” isn’t a valid placeholder. Use ${secret.NAME}, ${env.NAME}, ${data.column} or ${var.NAME}.", bundle: .app, locale: .app)
    case "flow.env.unknown":
      return String(localized: "flow.env.unknown", defaultValue: "No environment has the value \(p("name")). Add it to \(environmentsFileName()).", bundle: .app, locale: .app)
    case "flow.env.partial":
      return String(localized: "flow.env.partial", defaultValue: "The value \(p("name")) is missing in these environments: \(p("environments")).", bundle: .app, locale: .app)
    case "flow.environments.invalid":
      return String(localized: "flow.environments.invalid", defaultValue: "\(p("file")) can’t be read: \(p("reason")).", bundle: .app, locale: .app)
    case "flow.data.invalid":
      return String(localized: "flow.data.invalid", defaultValue: "\(p("path")) can’t be used as a data table: \(dataProblem(p("problem"), p)).", bundle: .app, locale: .app)
    case "flow.data.unknownColumn":
      return String(
        localized: "flow.data.unknownColumn",
        defaultValue: "\(p("path")) has no column “\(p("column"))”. Its columns are: \(p("columns")).", bundle: .app, locale: .app)
    case "flow.keys.invalid":
      return String(localized: "flow.keys.invalid", defaultValue: "“\(p("value"))” isn’t a key combination. Record the keys again.", bundle: .app, locale: .app)
    case "flow.drag.samePlace":
      return String(localized: "flow.drag.samePlace", defaultValue: "The drag ends where it starts. Choose another place to drop on.", bundle: .app, locale: .app)
    case "flow.point.invalid":
      return String(localized: "flow.point.invalid", defaultValue: "The point must be inside the window.", bundle: .app, locale: .app)
    case "flow.area.invalid":
      return String(localized: "flow.area.invalid", defaultValue: "The area must lie inside the image.", bundle: .app, locale: .app)
    case "flow.loop.emptyBody":
      return String(localized: "flow.loop.emptyBody", defaultValue: "Nothing inside yet. Add a block to run in the loop.", bundle: .app, locale: .app)
    case "flow.cycle":
      return String(localized: "flow.cycle", defaultValue: "These blocks lead round in a circle: \(p("blocks")). Use Repeat to run steps again.", bundle: .app, locale: .app)
    case "flow.block.unreachable":
      return String(localized: "flow.block.unreachable", defaultValue: "Nothing leads to this block, so it never runs.", bundle: .app, locale: .app)
    case "flow.noCheck":
      return String(localized: "flow.noCheck", defaultValue: "This test never checks the screen, so it can’t catch a wrong result. Add a Check block.", bundle: .app, locale: .app)
    case "flow.loop.noReturn":
      return String(localized: "flow.loop.noReturn", defaultValue: "The blocks inside don’t lead back to this loop.", bundle: .app, locale: .app)
    case "flow.input.invalid":
      return String(localized: "flow.input.invalid", defaultValue: "“\(p("name"))” can’t name an input. Use letters, digits and _, and don’t start with a digit.", bundle: .app, locale: .app)
    case "flow.input.missing":
      return String(localized: "flow.input.missing", defaultValue: "The block asks for \(p("name")). Fill it in below.", bundle: .app, locale: .app)
    case "flow.if.sameCondition":
      return String(localized: "flow.if.sameCondition", defaultValue: "An earlier condition of this block checks the same thing, so this one never leads anywhere.", bundle: .app, locale: .app)
    case "flow.var.unknown":
      return String(localized: "flow.var.unknown", defaultValue: "No block before this one saves a value named \(p("name")).", bundle: .app, locale: .app)
    case "flow.name.invalid":
      return String(localized: "flow.name.invalid", defaultValue: "“\(p("value"))” can’t be a name: start with a letter or _, then use only letters, digits and _.", bundle: .app, locale: .app)
    case "flow.name.duplicate":
      return String(localized: "flow.name.duplicate", defaultValue: "The name “\(p("value"))” is used twice in this block.", bundle: .app, locale: .app)
    case "flow.http.url":
      return String(localized: "flow.http.url", defaultValue: "“\(p("value"))” isn’t a web address. Start it with http:// or https://.", bundle: .app, locale: .app)
    case "flow.http.header":
      return String(localized: "flow.http.header", defaultValue: "“\(p("value"))” can’t be a header name: use letters, digits and -, with no spaces.", bundle: .app, locale: .app)
    case "flow.http.saveFrom":
      return String(localized: "flow.http.saveFrom", defaultValue: "“\(p("value"))” isn’t a part of the answer. Use status, body, body.field or header.Name.", bundle: .app, locale: .app)
    case "flow.value.notNumber":
      return String(localized: "flow.value.notNumber", defaultValue: "“\(p("value"))” isn’t a number, and greater or less than compares numbers.", bundle: .app, locale: .app)
    case "flow.script.inputType":
      let hint =
        switch p("type") {
        case "number": String(localized: "Write a number such as 3.5.")
        case "boolean": String(localized: "Write true or false.")
        case "image": String(localized: "Write the path of a PNG file.")
        default: String(localized: "Write four numbers from 0 to 1 as x,y,width,height, such as 0.1,0.2,0.5,0.3.")
        }
      return String(
        localized: "flow.script.inputType",
        defaultValue: "“\(p("value"))” isn’t a valid \(kindName(p("type"))) for the input \(p("name")). \(hint)", bundle: .app, locale: .app)
    case "flow.script.inputKind":
      return String(
        localized: "flow.script.inputKind",
        defaultValue: "The input \(p("name")) takes \(kindName(p("type"))), but \(p("variable")) is saved as \(p("from").split(separator: ", ").map { kindName(String($0)) }.joined(separator: ", ")).", bundle: .app, locale: .app)
    case "flow.dateTime.format":
      return String(
        localized: "flow.dateTime.format",
        defaultValue: "“\(p("value"))” isn’t a date format. Write it like \("%Y-%m-%d %H:%M"): a letter after a percent sign stands for a part of the date.", bundle: .app, locale: .app)
    default:
      return String(localized: "Unknown problem: \(issue.code)")
    }
  }

  /// What is wrong with a CSV data table, from the core's `data.*` code and its values.
  static func dataProblem(_ code: String, _ p: (String) -> String) -> String {
    switch code {
    case "data.unreadable":
      String(localized: "data.unreadable", defaultValue: "the file can’t be read (\(p("reason")))", bundle: .app, locale: .app)
    case "data.unwritable":
      String(localized: "data.unwritable", defaultValue: "the file can’t be saved (\(p("reason")))", bundle: .app, locale: .app)
    case "data.notText":
      String(localized: "data.notText", defaultValue: "the file isn’t text in UTF-8", bundle: .app, locale: .app)
    case "data.empty":
      String(localized: "data.empty", defaultValue: "the file is empty, and its first line must name the columns", bundle: .app, locale: .app)
    case "data.blankColumn":
      String(localized: "data.blankColumn", defaultValue: "column \(p("column")) of the first line has no name", bundle: .app, locale: .app)
    case "data.duplicateColumn":
      String(localized: "data.duplicateColumn", defaultValue: "the first line names the column “\(p("name"))” twice", bundle: .app, locale: .app)
    case "data.unclosedQuote":
      String(localized: "data.unclosedQuote", defaultValue: "the quote opened on line \(p("line")) is never closed", bundle: .app, locale: .app)
    case "data.strayQuote":
      String(
        localized: "data.strayQuote",
        defaultValue: "line \(p("line")) has a quote in the middle of a value; put the whole value in quotes and double the quote inside", bundle: .app, locale: .app)
    case "data.rowLength":
      String(
        localized: "data.rowLength",
        defaultValue: "line \(p("line")) has \(p("found")) values, but the first line names \(p("expected")) columns", bundle: .app, locale: .app)
    default:
      p("reason")
    }
  }

  /// Name of the setting a problem is about, from the first part of its `field`, e.g. `until.appears.image`.
  static func fieldLabel(_ field: String) -> String? {
    switch field.split(separator: ".").first.map(String.init) {
    case "name": String(localized: "Name")
    case "app": String(localized: "App")
    case "windowSize": String(localized: "Window Size")
    case "windowPosition": String(localized: "Window Position")
    case "text": String(localized: "Text")
    case "keys": String(localized: "Keys")
    case "files": String(localized: "Files")
    case "target": String(localized: "Target")
    case "into": String(localized: "Field")
    case "from": String(localized: "Drag From")
    case "to": String(localized: "Drop On")
    case "at": String(localized: "Over an element")
    case "amount": String(localized: "Amount")
    case "until", "reference": String(localized: "Reference")
    case "timeout": String(localized: "Time Limit")
    case "times": String(localized: "Times")
    case "file": String(localized: "Data File")
    case "test": String(localized: "Test")
    case "retries": String(localized: "Retries")
    case "value": String(localized: "Value")
    case "language": String(localized: "Language")
    case "code": String(localized: "Code")
    case "inputs": String(localized: "Inputs")
    case "outputs": String(localized: "Outputs")
    case "command": String(localized: "Command")
    case "folder": String(localized: "Folder")
    case "exitCode": String(localized: "Expected Exit Code")
    case "saveOutput": String(localized: "Save Output As")
    case "url": String(localized: "Address")
    case "headers": String(localized: "Headers")
    case "body": String(localized: "Body")
    case "expectStatus": String(localized: "Expected Status")
    case "save": String(localized: "Save From the Answer")
    case "path": String(localized: "File")
    case "query": String(localized: "Query")
    case "saveAs": String(localized: "Save First Value As")
    case "expect": String(localized: "Expect")
    case "link": String(localized: "Link")
    default: nil
    }
  }

  /// The message with the name of its setting in front, for lists outside the inspector.
  static func line(_ issue: Issue) -> String {
    guard let label = issue.param("field").flatMap(fieldLabel) else { return message(issue) }
    return String(localized: "\(label): \(message(issue))")
  }

  /// A kind of script value (`number`, `image`…) in words, for the middle of a sentence.
  private static func kindName(_ raw: String) -> String {
    let kind: ValueKind =
      switch raw {
      case "number": .number
      case "boolean": .boolean
      case "image": .image
      case "area": .area
      default: .text
      }
    return BlockInfo.valueKindName(kind).lowercased()
  }
}
