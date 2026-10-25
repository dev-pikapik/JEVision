import Foundation
import JEVisionMobile

/// Block settings and problems in words, for the block editor.
enum BlockText {
  /// The Mac's words for a problem of a block, from the catalog the Mac app shares.
  static func problem(_ problem: TestFlow.Problem) -> String {
    describeProblem(problem: JEVisionMobile.Problem(code: problem.code, warning: problem.warning, params: problem.params))
  }

  /// The settings of a block in sections: its own first, then one per element it looks for.
  static func sections(_ fields: [TestFlow.Field]) -> [(group: String?, fields: [TestFlow.Field])] {
    var sections: [(group: String?, fields: [TestFlow.Field])] = []
    for field in fields {
      let group = group(of: field.path)
      if let last = sections.indices.last, sections[last].group == group {
        sections[last].fields.append(field)
      } else {
        sections.append((group, [field]))
      }
    }
    return sections
  }

  /// `target` of `target.image`; `nil` for a setting of the block itself.
  static func group(of path: String) -> String? {
    path.firstIndex(of: ".").map { String(path[..<$0]) }
  }

  static func groupTitle(_ group: String) -> String {
    switch group {
    case "target": String(localized: "Target")
    case "from": String(localized: "Drag From")
    case "to": String(localized: "Drop On")
    case "into": String(localized: "Type Into")
    case "reference": String(localized: "Reference")
    default: group
    }
  }

  /// What the section of an element says about filling it in.
  static func groupFooter(_ group: String) -> String? {
    switch group {
    case "target", "from", "to":
      String(localized: "Find the element by a picture of the project or by words. Filling in one clears the other.")
    case "into":
      String(localized: "Find the field by a picture of the project or by words, or leave both empty to type where the cursor is.")
    default: nil
    }
  }

  static func label(_ path: String) -> String {
    switch path.split(separator: ".").last.map(String.init) ?? path {
    case "app": String(localized: "App")
    case "resetState": String(localized: "Start Fresh")
    case "newInstance": String(localized: "Open in a new copy of the app")
    case "closeAfter": String(localized: "Close after the test")
    case "image": String(localized: "Picture")
    case "description": String(localized: "In Words")
    case "threshold": String(localized: "Similarity")
    case "waitForChange": String(localized: "Wait until the screen changes")
    case "timeout": String(localized: "Timeout")
    case "button": String(localized: "Button")
    case "double": String(localized: "Double-click")
    case "direction": String(localized: "Direction")
    case "amount": String(localized: "Amount")
    case "unit": String(localized: "Unit")
    case "text": String(localized: "Text")
    case "keys": String(localized: "Keys")
    case "duration": String(localized: "Duration")
    case "times": String(localized: "Times")
    case "test": String(localized: "Test")
    case "file": String(localized: "File")
    case "saveAs": String(localized: "Save to Variable")
    case "format": String(localized: "Format")
    case "language": String(localized: "Language")
    case "timeZone": String(localized: "Time Zone")
    case "method": String(localized: "Method")
    case "url": String(localized: "URL")
    case "body": String(localized: "Body")
    case "query": String(localized: "Query")
    case "path": String(localized: "Path")
    case "command": String(localized: "Command")
    case "folder": String(localized: "Folder")
    case "code": String(localized: "Code")
    default: path
    }
  }

  /// What the field shows while empty.
  static func placeholder(_ field: TestFlow.Field) -> String {
    switch field.kind {
    case .duration: String(localized: "For example, 2s or 500ms")
    case .number: "0"
    default: ""
    }
  }

  /// A value of a choice as the Mac names it; methods and languages are written as they are.
  static func choice(_ value: String) -> String {
    switch value {
    case "left": String(localized: "Left")
    case "right": String(localized: "Right")
    case "middle": String(localized: "Middle")
    case "up": String(localized: "Up")
    case "down": String(localized: "Down")
    case "lines": String(localized: "Lines")
    case "pixels": String(localized: "Pixels")
    case "python3": "Python 3"
    case "node": "Node.js"
    case "appleScript": "AppleScript"
    default: value
    }
  }

  /// A group of the block library, as the Mac's library names it.
  static func libraryGroup(_ group: String) -> String {
    switch group {
    case "actions": String(localized: "Actions")
    case "waiting": String(localized: "Waiting")
    case "checks": String(localized: "Checks")
    case "logic": String(localized: "Conditions and Loops")
    case "subTests": String(localized: "Sub-Tests")
    case "data": String(localized: "Data and System")
    default: group
    }
  }
}
