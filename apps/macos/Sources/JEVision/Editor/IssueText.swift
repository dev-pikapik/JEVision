import Foundation
import JEVisionCore

/// Problems found by the core, in words: the core words them, in the language the app hands it.
enum IssueText {
  static func message(_ issue: Issue) -> String { issueSentence(issue: issue) }

  /// Name of the setting a problem is about, from the first part of its `field`, e.g. `until.appears.image`.
  static func fieldLabel(_ field: String) -> String? {
    switch field.split(separator: ".").first.map(String.init) {
    case "name": String(localized: "Name")
    case "app": String(localized: "App")
    case "windowSize": String(localized: "Window Size")
    case "windowPosition": String(localized: "Window Position")
    case "arguments": String(localized: "Arguments")
    case "environment": String(localized: "Environment Variables")
    case "element": String(localized: "Element")
    case "text": String(localized: "Text")
    case "image": String(localized: "Picture")
    case "content": String(localized: "What to Read")
    case "timeZone": String(localized: "Time Zone")
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
}
