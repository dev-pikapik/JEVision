import JEVisionCore
import SwiftUI

/// How a block type looks and is named in the editor. Keys are the `type` values of the file.
enum BlockInfo {
  static func symbol(_ type: String) -> String {
    switch type {
    case "openApp": "macwindow.badge.plus"
    case "openFile": "doc"
    case "click": "cursorarrow.click"
    case "type": "character.cursor.ibeam"
    case "pressKeys": "command"
    case "waitFor": "hourglass"
    case "check": "checkmark.seal"
    case "if": "arrow.triangle.branch"
    case "repeat": "repeat"
    case "useData": "tablecells"
    case "runTest": "rectangle.stack"
    default: "square.dashed"
    }
  }

  static func title(_ type: String) -> String {
    switch type {
    case "openApp": String(localized: "Open App")
    case "openFile": String(localized: "Open File")
    case "click": String(localized: "Click")
    case "type": String(localized: "Type Text")
    case "pressKeys": String(localized: "Press Keys")
    case "waitFor": String(localized: "Wait For")
    case "check": String(localized: "Check")
    case "if": String(localized: "If")
    case "repeat": String(localized: "Repeat")
    case "useData": String(localized: "Use Data")
    case "runTest": String(localized: "Run Test")
    default: type
    }
  }

  static func explanation(_ type: String) -> String {
    switch type {
    case "openApp": String(localized: "Launches an app and brings it to the front.")
    case "openFile": String(localized: "Opens files or web pages. Each file opens from a fresh copy, so the original stays intact.")
    case "click": String(localized: "Clicks an element found by how it looks, or a fixed point in the window.")
    case "type": String(localized: "Types text, optionally clicking into a field first.")
    case "pressKeys": String(localized: "Presses one key combination.")
    case "waitFor": String(localized: "Waits until something appears or disappears, or until the screen stops changing.")
    case "check": String(localized: "Fails the test unless the screen looks as expected.")
    case "if": String(localized: "Takes a different path depending on which reference the screen matches.")
    case "repeat": String(localized: "Runs the blocks inside it several times.")
    case "useData": String(localized: "Runs the blocks inside it once for each row of a CSV file.")
    case "runTest": String(localized: "Runs another test as a single step.")
    default: ""
    }
  }

  /// What the block does, short enough for one line of the palette.
  static func tagline(_ type: String) -> String {
    switch type {
    case "openApp": String(localized: "Launches an app")
    case "openFile": String(localized: "Opens a file or a web page")
    case "click": String(localized: "Clicks a button or a point")
    case "type": String(localized: "Types text into a field")
    case "pressKeys": String(localized: "Presses a key combination")
    case "waitFor": String(localized: "Waits for the screen to change")
    case "check": String(localized: "Fails unless the screen matches")
    case "if": String(localized: "Takes a path by what’s on screen")
    case "repeat": String(localized: "Repeats the steps inside")
    case "useData": String(localized: "Repeats the steps for each table row")
    case "runTest": String(localized: "Runs another test as one step")
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
    case .data: String(localized: "Data")
    }
  }

  /// Every block type in palette order, read once from the core.
  static let catalog: [BlockSpec] = blockCatalog()

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
    case .type: "type"
    case .pressKeys: "pressKeys"
    case .waitFor: "waitFor"
    case .check: "check"
    case .if: "if"
    case .repeat: "repeat"
    case .useData: "useData"
    case .runTest: "runTest"
    }
  }
}

extension Issue {
  func param(_ name: String) -> String? {
    params.first { $0.name == name }?.value
  }
}
