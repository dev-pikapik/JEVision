import JEVisionCore
import SwiftUI

/// How a block type looks and is named in the editor. Keys are the `type` values of the file.
enum BlockInfo {
  static func symbol(_ type: String) -> String {
    switch type {
    case "openApp": "macwindow.badge.plus"
    case "openFile": "doc"
    case "click": "cursorarrow.click"
    case "hover": "cursorarrow.rays"
    case "drag": "cursorarrow.and.square.on.square.dashed"
    case "scroll": "scroll"
    case "type": "character.cursor.ibeam"
    case "pressKeys": "command"
    case "waitFor": "hourglass"
    case "check": "checkmark.seal"
    case "if": "arrow.triangle.branch"
    case "repeat": "repeat"
    case "useData": "tablecells"
    case "runTest": "rectangle.stack"
    case "http": "network"
    case "database": "cylinder"
    case "checkFile": "doc.badge.clock"
    case "command": "terminal"
    case "script": "curlybraces"
    default: "square.dashed"
    }
  }

  static func title(_ type: String) -> String {
    switch type {
    case "openApp": String(localized: "Open App")
    case "openFile": String(localized: "Open File")
    case "click": String(localized: "Click")
    case "hover": String(localized: "Hover")
    case "drag": String(localized: "Drag")
    case "scroll": String(localized: "Scroll")
    case "type": String(localized: "Type Text")
    case "pressKeys": String(localized: "Press Keys")
    case "waitFor": String(localized: "Wait For")
    case "check": String(localized: "Check")
    case "if": String(localized: "If")
    case "repeat": String(localized: "Repeat")
    case "useData": String(localized: "Use Data")
    case "runTest": String(localized: "Run Test")
    case "http": String(localized: "HTTP Request")
    case "database": String(localized: "Database")
    case "checkFile": String(localized: "Wait For File")
    case "command": String(localized: "Run Command")
    case "script": String(localized: "Script")
    default: type
    }
  }

  static func explanation(_ type: String) -> String {
    switch type {
    case "openApp": String(localized: "Launches an app and brings it to the front.")
    case "openFile": String(localized: "Opens files or web pages. Each file opens from a fresh copy, so the original stays intact.")
    case "click": String(localized: "Clicks an element found by how it looks, or a fixed point in the window.")
    case "hover": String(localized: "Moves the pointer onto an element and leaves it there, to show a tooltip or a menu that opens on hover.")
    case "drag": String(localized: "Presses on one place, moves while holding the button and lets go on another: to move, resize or drop something.")
    case "scroll": String(localized: "Turns the scroll wheel over the window or over an element, by lines or by pixels.")
    case "type": String(localized: "Types text, optionally clicking into a field first.")
    case "pressKeys": String(localized: "Presses one key combination.")
    case "waitFor": String(localized: "Waits until something appears or disappears, or until the screen stops changing.")
    case "check": String(localized: "Fails the test unless the screen looks as expected.")
    case "if": String(localized: "Takes a different path depending on what the screen shows or what a saved value is.")
    case "repeat": String(localized: "Runs the blocks inside it several times.")
    case "useData": String(localized: "Runs the blocks inside it once for each row of a CSV file.")
    case "runTest": String(localized: "Runs another test as a single step.")
    case "http": String(localized: "Sends a request to a web address, checks the answer and saves parts of it for later blocks.")
    case "database": String(localized: "Reads an SQLite database with a query, checks the result and saves it for later blocks.")
    case "checkFile": String(localized: "Waits until a file appears, disappears, changes or contains a text.")
    case "command": String(localized: "Runs a Terminal command with your permissions and checks how it ended.")
    case "script": String(localized: "Runs a short program in sh, zsh, Python, JavaScript or AppleScript, with your permissions.")
    default: ""
    }
  }

  /// What the block does, short enough for one line of the palette.
  static func tagline(_ type: String) -> String {
    switch type {
    case "openApp": String(localized: "Launches an app")
    case "openFile": String(localized: "Opens a file or a web page")
    case "click": String(localized: "Clicks a button or a point")
    case "hover": String(localized: "Points at something without clicking")
    case "drag": String(localized: "Drags from one place to another")
    case "scroll": String(localized: "Scrolls the page or a list")
    case "type": String(localized: "Types text into a field")
    case "pressKeys": String(localized: "Presses a key combination")
    case "waitFor": String(localized: "Waits for the screen to change")
    case "check": String(localized: "Fails unless the screen matches")
    case "if": String(localized: "Takes a path by the screen or a value")
    case "repeat": String(localized: "Repeats the steps inside")
    case "useData": String(localized: "Repeats the steps for each table row")
    case "runTest": String(localized: "Runs another test as one step")
    case "http": String(localized: "Sends a request to a web service")
    case "database": String(localized: "Reads an SQLite database")
    case "checkFile": String(localized: "Waits for a file to change")
    case "command": String(localized: "Runs a Terminal command")
    case "script": String(localized: "Runs a short program")
    default: ""
    }
  }

  static func color(_ group: BlockGroup) -> Color {
    switch group {
    case .actions: .blue
    case .waiting: .orange
    case .checks: .teal
    case .logic: .purple
    case .subTests: .indigo
    case .data: .green
    }
  }

  static func groupTitle(_ group: BlockGroup) -> String {
    switch group {
    case .actions: String(localized: "Actions")
    case .waiting: String(localized: "Waiting")
    case .checks: String(localized: "Checks")
    case .logic: String(localized: "Conditions and Loops")
    case .subTests: String(localized: "Sub-Tests")
    case .data: String(localized: "Data and System")
    }
  }

  /// Every block type in palette order, read once from the core.
  static let catalog: [BlockSpec] = blockCatalog()

  static let languages: [ScriptLanguage] = [.sh, .zsh, .python3, .node, .appleScript]

  static func languageName(_ language: ScriptLanguage) -> String {
    switch language {
    case .sh: "sh"
    case .zsh: "zsh"
    case .python3: "Python 3"
    case .node: String(localized: "JavaScript (Node.js)")
    case .appleScript: "AppleScript"
    }
  }

  static let methods: [HttpMethod] = [.get, .post, .put, .patch, .delete, .head]

  static func methodName(_ method: HttpMethod) -> String {
    switch method {
    case .get: "GET"
    case .post: "POST"
    case .put: "PUT"
    case .patch: "PATCH"
    case .delete: "DELETE"
    case .head: "HEAD"
    }
  }

  static func color(ofType type: String) -> Color {
    catalog.first { $0.typeName == type }.map { color($0.group) } ?? .gray
  }
}

extension BlockKind {
  /// What the editor calls way out `edge` of this block: Yes and No for an `if` with one
  /// condition, Condition 1, 2… and Otherwise once it has several; `nil` for the
  /// edges of other blocks.
  func branchName(_ edge: String) -> String? {
    guard case .if(_, _, let elseIf, _) = self else { return nil }
    if edge == "then" { return elseIf.isEmpty ? String(localized: "Yes") : String(localized: "Condition \(1)") }
    if edge == "else" { return elseIf.isEmpty ? String(localized: "No") : String(localized: "Otherwise") }
    guard let n = elseIfIndex(edge: edge).map(Int.init), n < elseIf.count else { return nil }
    return String(localized: "Condition \(n + 2)")
  }

  /// The `type` value of the block in the file.
  var typeName: String {
    switch self {
    case .openApp: "openApp"
    case .openFile: "openFile"
    case .click: "click"
    case .hover: "hover"
    case .drag: "drag"
    case .scroll: "scroll"
    case .type: "type"
    case .pressKeys: "pressKeys"
    case .waitFor: "waitFor"
    case .check: "check"
    case .if: "if"
    case .repeat: "repeat"
    case .useData: "useData"
    case .runTest: "runTest"
    case .script: "script"
    case .command: "command"
    case .http: "http"
    case .checkFile: "checkFile"
    case .database: "database"
    }
  }
}

extension Issue {
  func param(_ name: String) -> String? {
    params.first { $0.name == name }?.value
  }
}
