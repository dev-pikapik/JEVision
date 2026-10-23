import JEVisionCore
import SwiftUI

/// How a test on the board stands, by shape and colour: running now, waiting for its turn in a
/// batch, how it last ran, or why it can't run. One look for the test's card, the board from afar,
/// a frame's summary and the title of an open test.
struct BoardTestStatus: Equatable {
  enum Kind: Equatable {
    /// The test file is no longer in the project.
    case missing
    case unreadable
    /// Readable, but with problems that keep it from running.
    case broken
    case running
    case waiting
    case notRun
    case ran(RunStatus)
  }

  let kind: Kind

  init(_ kind: Kind) {
    self.kind = kind
  }

  init(info: MapTest?, last: SavedRun?, isRunning: Bool, isWaiting: Bool = false, isMissing: Bool) {
    if isMissing {
      kind = .missing
    } else if isRunning {
      kind = .running
    } else if isWaiting {
      kind = .waiting
    } else if let info, !info.readable {
      kind = .unreadable
    } else if let info, info.errors > 0 {
      kind = .broken
    } else if let last {
      kind = .ran(last.status)
    } else {
      kind = .notRun
    }
  }

  var symbol: String {
    switch kind {
    case .missing: "questionmark.folder"
    case .unreadable: "lock.doc.fill"
    case .broken: "exclamationmark.octagon.fill"
    case .running: "play.fill"
    case .waiting: "clock"
    case .notRun: "circle.dashed"
    case .ran(let status): status.symbol
    }
  }

  var color: Color {
    switch kind {
    case .unreadable, .broken: .red
    case .running: .accentColor
    case .missing, .waiting, .notRun: .secondary
    case .ran(let status): status.color
    }
  }

  var text: String {
    switch kind {
    case .missing: String(localized: "Not in the project any more")
    case .unreadable: String(localized: "Can’t be read")
    case .broken: String(localized: "Can’t run: fix the problems in the test")
    case .running: String(localized: "Running…")
    case .waiting: String(localized: "Waiting for its turn")
    case .notRun: String(localized: "Not run yet")
    case .ran(let status): status.title
    }
  }

  /// The colour a test's card takes on the board from afar, when how it stands is worth a colour.
  var tint: Color? {
    switch kind {
    case .missing, .waiting, .notRun: nil
    default: color
    }
  }

  /// How much the status matters in a summary: the lowest number leads.
  fileprivate var rank: Int {
    switch kind {
    case .running: 0
    case .ran(.failed): 1
    case .broken: 2
    case .unreadable: 3
    case .ran(.flaky): 4
    case .ran(.blocked): 5
    case .ran(.quarantined): 6
    case .waiting: 7
    case .ran(.cancelled): 8
    case .ran(.unknown): 9
    case .ran(.unchecked): 10
    case .missing: 11
    case .notRun: 12
    case .ran(.passed): 13
    }
  }
}

/// What a frame says of the tests inside it.
struct BoardRollup: Equatable {
  private(set) var tests = 0
  private(set) var passed = 0
  private(set) var failed = 0
  private(set) var running = 0
  /// The status among them that matters most, which the frame wears as its badge.
  private(set) var lead: BoardTestStatus?

  init(_ statuses: [BoardTestStatus]) {
    tests = statuses.count
    for status in statuses {
      switch status.kind {
      case .ran(.passed): passed += 1
      case .ran(.failed): failed += 1
      case .running: running += 1
      default: break
      }
    }
    lead = statuses.min { $0.rank < $1.rank }
  }

  /// "3 of 5 passed, 1 failed": empty for a frame with no tests.
  var text: String {
    guard tests > 0 else { return "" }
    var parts = [String(localized: "\(passed) of \(tests) passed")]
    if failed > 0 { parts.append(String(localized: "\(failed) failed")) }
    if running > 0 { parts.append(String(localized: "\(running) running")) }
    return parts.formatted(.list(type: .and, width: .narrow))
  }
}

/// Where the standing of each test comes from: the names the project has, what the core read from
/// each test, how each last ran, and the batch running now.
struct BoardStatusSource {
  var names: [String: String] = [:]
  var map: [String: MapTest] = [:]
  var latest: [String: SavedRun] = [:]
  /// The tests running now, relative to the project.
  var running: Set<String> = []
  /// Tests of the batch running now that haven't started.
  var waiting: Set<String> = []

  func status(of file: String) -> BoardTestStatus {
    BoardTestStatus(
      info: map[file], last: latest[file], isRunning: running.contains(file), isWaiting: waiting.contains(file),
      isMissing: names[file] == nil)
  }
}

/// What the board in view looks like from afar, worked out once per change of the board or of how
/// the tests ran, so drawing and the minimap don't repeat it for every frame of a pan.
struct BoardLook: Equatable {
  /// How each test on the board stands, by test file.
  private(set) var status: [String: BoardTestStatus] = [:]
  /// What each frame sums up, by frame id.
  private(set) var rollups: [String: BoardRollup] = [:]
  /// The tests on the board that are in the project, each once, in board order.
  private(set) var tests: [String] = []
  /// How each shown node looks from afar, by its position in the scene's layout.
  private(set) var glances: [BoardGlance] = []

  init() {}

  init(_ scene: BoardScene, _ source: BoardStatusSource) {
    var seen: Set<String> = []
    for node in scene.board.nodes {
      guard let file = Self.test(node), source.names[file] != nil, seen.insert(file).inserted else { continue }
      tests.append(file)
    }
    for file in tests { status[file] = source.status(of: file) }
    for (frame, files) in scene.frameTests {
      let inside = files.compactMap { status[$0] }
      if !inside.isEmpty { rollups[frame] = BoardRollup(inside) }
    }
    glances = scene.layout.nodes.map { laid in
      guard let node = scene.nodes[laid.id] else { return BoardGlance(shape: .card, title: "", color: nil) }
      return glance(of: node, scene: scene, source: source)
    }
  }

  private func glance(of node: BoardNode, scene: BoardScene, source: BoardStatusSource) -> BoardGlance {
    let tint = BoardColor.color(node.color)
    switch node.kind {
    case .group(let label, _, _):
      return BoardGlance(shape: .frame, title: label, color: tint, badge: rollups[node.id]?.lead)
    case .text(let text):
      return BoardGlance(shape: .note, title: String(text.prefix { $0 != "\n" }), color: tint)
    case .link(let url):
      return BoardGlance(shape: .card, title: URL(string: url)?.host() ?? url, color: tint)
    case .file(let file):
      let last = (file as NSString).lastPathComponent
      if ProjectScanner.isTestFile(file) {
        let standing = status[file] ?? source.status(of: file)
        return BoardGlance(
          shape: .card, title: source.names[file] ?? ProjectScanner.testName(last), color: standing.tint ?? tint,
          badge: standing)
      }
      if scene.images.contains(node.id) {
        return BoardGlance(shape: .picture, title: last, color: tint)
      }
      return BoardGlance(shape: .card, title: ProjectBoards.name(of: file) ?? file, color: tint)
    }
  }

  static func test(_ node: BoardNode) -> String? {
    if case .file(let file) = node.kind, ProjectScanner.isTestFile(file) { file } else { nil }
  }
}
