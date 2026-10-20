import SwiftUI

/// A rounded colour tile with a white symbol, the same as on the Mac. A solid fill, not glass:
/// tiles are content.
struct IconTile: View {
  let symbol: String
  let color: Color
  var size: CGFloat = 28

  var body: some View {
    Image(systemName: symbol)
      .font(.system(size: size * 0.48, weight: .semibold))
      .foregroundStyle(.white)
      .frame(width: size, height: size)
      .background(color.gradient, in: .rect(cornerRadius: size * 0.27))
      .accessibilityHidden(true)
  }
}

/// How a block type looks and is named, as in the Mac editor. Keys are the test file's `type`.
enum BlockInfo {
  static func symbol(_ kind: String) -> String {
    switch kind {
    case "openApp": "macwindow.badge.plus"
    case "openFile": "doc"
    case "click": "cursorarrow.click"
    case "hover": "cursorarrow.rays"
    case "drag": "cursorarrow.and.square.on.square.dashed"
    case "scroll": "scroll"
    case "type": "character.cursor.ibeam"
    case "pressKeys": "command"
    case "waitFor": "hourglass"
    case "pause": "timer"
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
    case "clipboard": "doc.on.clipboard"
    case "dateTime": "calendar.badge.clock"
    default: "square.dashed"
    }
  }

  static func title(_ kind: String) -> String {
    switch kind {
    case "openApp": String(localized: "Open App")
    case "openFile": String(localized: "Open File")
    case "click": String(localized: "Click")
    case "hover": String(localized: "Hover")
    case "drag": String(localized: "Drag")
    case "scroll": String(localized: "Scroll")
    case "type": String(localized: "Type Text")
    case "pressKeys": String(localized: "Press Keys")
    case "waitFor": String(localized: "Wait For")
    case "pause": String(localized: "Pause")
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
    case "clipboard": String(localized: "Clipboard")
    case "dateTime": String(localized: "Date and Time")
    default: kind
    }
  }

  static func color(_ kind: String) -> Color {
    switch kind {
    case "openApp", "openFile", "click", "hover", "drag", "scroll", "type", "pressKeys": .blue
    case "waitFor", "pause", "check": .orange
    case "if", "repeat", "runTest": .purple
    case "useData", "http", "database", "checkFile", "command", "script", "clipboard", "dateTime": .green
    default: .gray
    }
  }
}

/// A result told by shape as well as colour: circle passed, square failed, triangle flaky.
struct VerdictStyle {
  let title: String
  let symbol: String
  let color: Color

  init(_ verdict: Verdict, flaky: Bool = false) {
    switch verdict {
    case .passed where flaky:
      (title, symbol, color) = (String(localized: "Flaky"), "exclamationmark.triangle.fill", .orange)
    case .passed: (title, symbol, color) = (String(localized: "Passed"), "checkmark.circle.fill", .green)
    case .failed: (title, symbol, color) = (String(localized: "Failed"), "xmark.square.fill", .red)
    case .cancelled: (title, symbol, color) = (String(localized: "Stopped"), "stop.circle.fill", .secondary)
    case .blocked: (title, symbol, color) = (String(localized: "Blocked"), "minus.circle.fill", .yellow)
    case .unknown: (title, symbol, color) = (String(localized: "Unknown status"), "questionmark.circle.fill", .secondary)
    }
  }

  init(_ status: StepStatus) {
    switch status {
    case .passed: (title, symbol, color) = (String(localized: "Passed"), "checkmark.circle.fill", .green)
    case .failed: (title, symbol, color) = (String(localized: "Failed"), "xmark.square.fill", .red)
    case .baselineCreated:
      (title, symbol, color) = (String(localized: "New reference saved"), "plus.circle.fill", .blue)
    case .cancelled: (title, symbol, color) = (String(localized: "Stopped"), "stop.circle.fill", .secondary)
    case .unknown: (title, symbol, color) = (String(localized: "Unknown status"), "questionmark.circle.fill", .secondary)
    }
  }

  var image: some View {
    Image(systemName: symbol)
      .foregroundStyle(color)
      .accessibilityLabel(title)
  }
}
