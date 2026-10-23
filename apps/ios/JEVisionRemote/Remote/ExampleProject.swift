import UIKit

/// The project on the example Mac: its tests as a Mac lays them out, how their runs go, and
/// the pictures of the one that fails.
enum ExampleProject {
  static let name = "Acme Mail"
  static let signIn = "login/sign-in.flow.json"
  static let people = "people.flow.json"

  static func entries() -> [TestEntry] {
    [
      TestEntry(path: "login", name: "login", isFolder: true),
      TestEntry(path: signIn, name: String(localized: "Sign in with valid credentials"), isFolder: false),
      TestEntry(path: people, name: String(localized: "Greet every person"), isFolder: false),
    ]
  }

  static func flow(_ path: String) -> TestFlow? {
    switch path {
    case signIn: signInFlow
    case people: peopleFlow
    default: nil
    }
  }

  /// What the block works with while it runs, its placeholders filled from the row.
  static func runningTitle(of block: TestFlow.Block, row: String?) -> String? {
    if block.kind == "type", let row { return String(localized: "Hello, \(row)!") }
    return block.title.isEmpty ? nil : block.title
  }

  /// Sign-in always fails at its last check: an update banner moved the message list.
  static func failure(of block: TestFlow.Block, in path: String) -> Failure? {
    guard path == signIn, block.id == "messages" else { return nil }
    return Failure(
      code: "run.check.mismatch",
      params: ["block": block.id, "image": block.title, "similarity": "0.82", "threshold": "0.9"])
  }

  /// What the way an `if` takes looks for; the example's only `if` always finds it.
  static func branch(of block: TestFlow.Block, in path: String) -> String? {
    path == people && block.id == "ready" ? String(localized: "An empty document") : nil
  }

  static func picture(_ name: String) -> Data? {
    switch name {
    case "messages-after.png": inbox(banner: true)
    case "messages-expected.png": inbox(banner: false)
    default: nil
    }
  }

  private static var signInFlow: TestFlow {
    let steps = [
      ("open", "openApp", ""), ("email", "type", "demo@example.com"), ("password", "type", "${secret.ACME_PASSWORD}"),
      ("submit", "click", "refs/sign-in-button.png"), ("inbox", "waitFor", "refs/inbox.png"),
      ("messages", "check", "refs/message-list.png"),
    ]
    let blocks = steps.enumerated().map { index, step in
      TestFlow.Block(id: step.0, kind: step.1, title: step.2, area: CGRect(x: 16, y: 48 + index * 112, width: 208, height: 64))
    }
    return TestFlow(
      name: String(localized: "Sign in with valid credentials"),
      description: String(localized: "Signs in with a known account and checks that the inbox appears."),
      app: "com.example.AcmeMail", start: "open", size: CGSize(width: 240, height: 720), blocks: blocks, frames: [],
      edges: zip(blocks, blocks.dropFirst()).map { from, to in
        TestFlow.Edge(from: from.id, edge: "next", to: to.id, points: [CGPoint(x: 120, y: from.area!.maxY), CGPoint(x: 120, y: to.area!.minY)])
      },
      priority: "critical", testType: "smoke", status: "ready")
  }

  private static var peopleFlow: TestFlow {
    let day = Calendar.current.startOfDay(for: Date())
    let time = { (days: Double, hours: Double) in day.addingTimeInterval((hours - days * 24) * 3600) }
    return TestFlow(
      name: String(localized: "Greet every person"),
      description: String(localized: "Types a greeting for each person in people.csv."),
      app: "com.apple.TextEdit", start: "open", size: CGSize(width: 496, height: 560),
      blocks: [
        .init(id: "open", kind: "openApp", title: "", area: CGRect(x: 56, y: 48, width: 208, height: 64)),
        .init(id: "ready", kind: "if", title: "", area: CGRect(x: 56, y: 160, width: 208, height: 64)),
        .init(id: "rows", kind: "useData", title: "people.csv", area: CGRect(x: 32, y: 288, width: 208, height: 64)),
        .init(id: "type", kind: "type", title: String(localized: "Hello, ${data.name}!"), area: CGRect(x: 32, y: 400, width: 208, height: 64)),
      ],
      frames: [.init(block: "rows", area: CGRect(x: 16, y: 272, width: 240, height: 240))],
      edges: [
        .init(from: "open", edge: "next", to: "ready", points: [CGPoint(x: 160, y: 112), CGPoint(x: 160, y: 160)]),
        .init(
          from: "ready", edge: "then", to: "rows",
          points: [CGPoint(x: 108, y: 224), CGPoint(x: 108, y: 240), CGPoint(x: 136, y: 240), CGPoint(x: 136, y: 272)]),
        .init(from: "rows", edge: "body", to: "type", points: [CGPoint(x: 136, y: 352), CGPoint(x: 136, y: 400)]),
        .init(
          from: "type", edge: "next", to: "rows",
          points: [
            CGPoint(x: 136, y: 464), CGPoint(x: 136, y: 496), CGPoint(x: 24, y: 496), CGPoint(x: 24, y: 320),
            CGPoint(x: 32, y: 320),
          ]),
      ],
      rows: ["Anna", "Boris", "Chen"], priority: "high", testType: "regression", status: "ready", author: "Anna Weber",
      notes: [
        TestNote(
          id: "greeting",
          text: String(localized: "**Check** the greeting for every row. The wording is in [the ticket](https://example.com/tickets/42)."),
          author: "Anna Weber", time: time(3, 9.5), anchor: .block("rows"), area: CGRect(x: 288, y: 144, width: 192, height: 128),
          replyCount: 2,
          replies: [
            .init(id: "r1", author: "Boris Keller", time: time(3, 10), text: String(localized: "Chen has no email yet, the greeting still works.")),
            .init(id: "r2", author: "Anna Weber", time: time(3, 10.3), text: String(localized: "Fine, **keep** it.")),
          ],
          resolved: nil, attachments: [.file("greeting.png"), .link(URL(string: "https://example.com/tickets/42")!)]),
        TestNote(
          id: "blank", text: String(localized: "TextEdit starts with a **blank** document."), author: "Boris Keller",
          time: time(4, 16), anchor: .block("open"), area: CGRect(x: 288, y: 16, width: 192, height: 96), replyCount: 0,
          replies: [], resolved: .init(by: "Boris Keller", time: time(4, 17)), attachments: []),
        TestNote(
          id: "timing", text: String(localized: "Runs about *40 seconds* for three rows."), author: "Chen Li",
          time: time(3, 11), anchor: .test, area: CGRect(x: 288, y: 304, width: 192, height: 96), replyCount: 0,
          replies: [], resolved: nil, attachments: []),
      ])
  }

  /// Acme Mail's window as the check saw it: with `banner`, an update notice pushed the
  /// messages down, which the reference doesn't have.
  private static func inbox(banner: Bool) -> Data {
    let size = CGSize(width: 480, height: 300)
    let format = UIGraphicsImageRendererFormat()
    format.scale = 2
    return UIGraphicsImageRenderer(size: size, format: format).pngData { context in
      let fill = { (color: UIColor, rect: CGRect, radius: CGFloat) in
        color.setFill()
        UIBezierPath(roundedRect: rect, cornerRadius: radius).fill()
      }
      fill(UIColor(white: 0.98, alpha: 1), CGRect(origin: .zero, size: size), 0)
      fill(UIColor(white: 0.9, alpha: 1), CGRect(x: 0, y: 0, width: size.width, height: 28), 0)
      for (index, color) in [UIColor.systemRed, .systemYellow, .systemGreen].enumerated() {
        fill(color, CGRect(x: 12 + CGFloat(index) * 18, y: 9, width: 10, height: 10), 5)
      }
      fill(UIColor(white: 0.93, alpha: 1), CGRect(x: 0, y: 28, width: 120, height: size.height - 28), 0)
      for index in 0..<4 {
        fill(UIColor(white: 0.8, alpha: 1), CGRect(x: 16, y: 48 + CGFloat(index) * 26, width: 72 - CGFloat(index % 2) * 18, height: 10), 5)
      }
      var y: CGFloat = 40
      if banner {
        fill(.systemOrange, CGRect(x: 132, y: y, width: size.width - 144, height: 36), 8)
        y += 48
      }
      for index in 0..<5 where y + 40 <= size.height {
        fill(UIColor.systemBlue.withAlphaComponent(0.3), CGRect(x: 136, y: y + 4, width: 28, height: 28), 14)
        fill(UIColor(white: 0.55, alpha: 1), CGRect(x: 176, y: y + 6, width: 140 - CGFloat(index % 3) * 20, height: 9), 4.5)
        fill(UIColor(white: 0.82, alpha: 1), CGRect(x: 176, y: y + 22, width: 260 - CGFloat(index % 2) * 40, height: 8), 4)
        y += 48
      }
    }
  }
}
