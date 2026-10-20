import Foundation

/// The test card's fields in words, worded as the Mac's test details word them.
enum CardText {
  struct Fact: Hashable, Identifiable {
    let label: String
    let value: String
    var id: String { label }
  }

  /// What the card says about the test, in the Mac's order; fields the test doesn't have are left out.
  static func facts(of flow: TestFlow) -> [Fact] {
    [
      flow.testType.map { Fact(label: String(localized: "Type"), value: testType($0)) },
      flow.priority.map { Fact(label: String(localized: "Priority"), value: priority($0)) },
      flow.status.map { Fact(label: String(localized: "Status"), value: status($0)) },
      flow.author.flatMap { $0.isEmpty ? nil : Fact(label: String(localized: "Author"), value: $0) },
    ].compactMap(\.self)
  }

  // A value from a newer Mac that this version doesn't know is shown as it is written.

  static func testType(_ tag: String) -> String {
    switch tag {
    case "smoke": String(localized: "Smoke")
    case "regression": String(localized: "Regression")
    case "acceptance": String(localized: "Acceptance")
    default: tag
    }
  }

  static func priority(_ tag: String) -> String {
    switch tag {
    case "critical": String(localized: "Critical")
    case "high": String(localized: "High")
    case "medium": String(localized: "Medium")
    case "low": String(localized: "Low")
    default: tag
    }
  }

  static func status(_ tag: String) -> String {
    switch tag {
    case "draft": String(localized: "Draft")
    case "ready": String(localized: "Ready")
    case "outdated": String(localized: "Outdated")
    default: tag
    }
  }
}
