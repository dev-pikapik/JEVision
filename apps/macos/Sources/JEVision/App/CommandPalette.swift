import JEVisionCore
import SwiftUI

/// What the command palette does when an entry is chosen.
enum PaletteAction: Hashable {
  case newTest, describeTest, openProject, settings
  case run, runStepByStep, runFromSelection, runAll, record, schedules
  case addBlock(String), addCondition, arrange, disconnect
  /// Adds a template or a custom block of the project.
  case addFromLibrary(LibraryEntry)
  /// Saves the selected blocks to the library.
  case saveToLibrary(LibraryEntry.Kind)
  case openReport, reportBug, testStatistics, projectStatistics
  case newBoard, addToBoard(BoardAddition)
  case commit, newPullRequest, pullRequests, pull, push
  /// Opens the test with this sidebar id.
  case openTest(String)
  /// Shows the board with this name.
  case showBoard(String)
  /// Shows the item with this id on the board in view.
  case showOnBoard(String)
}

/// One line of the command palette: a command, a block to add or a test to open.
struct PaletteEntry: Identifiable, Hashable {
  enum Kind: Hashable { case command, block, test, board }

  let action: PaletteAction
  let kind: Kind
  let title: String
  /// One line under the title: what the command does, the block's purpose, the test's folder.
  let detail: String
  let symbol: String
  let color: Color
  let shortcut: KeyboardShortcut?
  /// Other words the entry is found by.
  let keywords: String

  var id: PaletteAction { action }

  static func == (a: Self, b: Self) -> Bool { a.action == b.action }
  func hash(into hasher: inout Hasher) { hasher.combine(action) }
}

/// What can be done right now, as the command palette sees it.
struct PaletteState {
  var hasProject = false
  /// A test is open in the editor.
  var hasTest = false
  /// Runs and recordings can start: nothing runs or records and a test is open.
  var canRun = false
  var selection: String?
  var selectionIsIf = false
  var selectionHasLinks = false
  /// Blocks are selected on the canvas.
  var hasPickedBlocks = false
  var hasTests = false
  var hasLastRun = false
  /// The newest run of the open test failed.
  var lastRunFailed = false
  var hasRuns = false
  var git = Git()
  /// Tests of the project, as `(sidebar id, name, folder, what else finds it)`.
  var tests: [(id: String, name: String, folder: String, keywords: String)] = []
  var boards: [String] = []
  /// The board shown in the middle of the window.
  var boardInView: String?
  /// Some test of the project isn't on the board in view yet.
  var boardLacksTests = false
  /// Frames, notes and links on the board in view, as `(id, title, what it is)`.
  var boardItems: [(id: String, title: String, kind: String)] = []
  /// The project's templates and custom blocks.
  var library: [LibraryEntry] = []

  struct Git {
    var isTracked = false
    var hasChanges = false
    var hasCommits = false
    var hasUpstream = false
    var hasRemote = false
    var onGitHub = false
    var isBusy = false
  }
}

/// The entries of the command palette and how a search orders them.
enum CommandPalette {
  /// Everything that can be done in `state`: commands, then blocks to add, then tests to open,
  /// then boards and what is on the board in view.
  static func entries(_ state: PaletteState) -> [PaletteEntry] {
    commands(state) + blocks(state) + library(state) + tests(state) + boards(state)
  }

  /// `entries` that match `query`, best first; all of them for an empty query.
  static func search(_ query: String, in entries: [PaletteEntry]) -> [PaletteEntry] {
    let candidates = entries.map { SearchCandidate(title: $0.title, keywords: $0.keywords) }
    return rankSearch(query: query, candidates: candidates).map { entries[Int($0)] }
  }

  private static func commands(_ state: PaletteState) -> [PaletteEntry] {
    var list: [PaletteEntry] = []
    func add(
      _ action: PaletteAction, _ title: String, _ detail: String, symbol: String, shortcut: KeyboardShortcut? = nil,
      keywords: String = "", if condition: Bool = true
    ) {
      guard condition else { return }
      list.append(PaletteEntry(
        action: action, kind: .command, title: title, detail: detail, symbol: symbol, color: .accentColor,
        shortcut: shortcut, keywords: [detail, keywords].joined(separator: " ")))
    }
    let git = state.git
    add(
      .run, String(localized: "Run"), String(localized: "Runs the open test from the start."), symbol: "play.fill",
      shortcut: Shortcuts.run, keywords: String(localized: "start play test"), if: state.canRun)
    add(
      .record, String(localized: "Record"), String(localized: "Records steps by using the app."),
      symbol: "record.circle", shortcut: Shortcuts.record, if: state.canRun)
    add(
      .addCondition, state.selectionIsIf ? String(localized: "Add Condition") : String(localized: "Add If"),
      state.selectionIsIf
        ? String(localized: "Adds one more way out to the selected If.")
        : String(localized: "Adds an If after the selected block, so the test can take different paths."),
      symbol: "arrow.triangle.branch", shortcut: Shortcuts.addCondition,
      keywords: String(localized: "if condition branch scenario path"), if: state.hasTest)
    add(
      .runFromSelection, String(localized: "Run from Selected Block"),
      String(localized: "Runs the test starting at the selected block."), symbol: "arrow.down.to.line",
      shortcut: Shortcuts.runFromSelection, if: state.canRun && state.selection != nil)
    add(
      .runStepByStep, String(localized: "Run Step by Step"), String(localized: "Pauses after every block."),
      symbol: "forward.frame", shortcut: Shortcuts.runStepByStep, keywords: String(localized: "debug"),
      if: state.canRun)
    add(
      .openReport, String(localized: "Open Last Report"),
      String(localized: "Shows how the last run of the open test went."), symbol: "doc.text.magnifyingglass",
      keywords: String(localized: "result failure screenshot reference"), if: state.hasLastRun)
    add(
      .reportBug, String(localized: "Report Bug…"),
      String(localized: "Files the last failure of the open test as an issue on GitHub."), symbol: "ladybug",
      keywords: String(localized: "issue github failure bug report"), if: state.lastRunFailed)
    add(
      .testStatistics, String(localized: "Test Statistics"),
      String(localized: "Pass rate, durations and where the open test fails, over all its runs."),
      symbol: "chart.bar.xaxis", keywords: String(localized: "stats history trend flaky duration"),
      if: state.hasLastRun)
    add(
      .disconnect, String(localized: "Disconnect Block"),
      String(localized: "Removes the links into and out of the selected block."), symbol: "scissors",
      shortcut: Shortcuts.disconnect, keywords: String(localized: "unlink link arrow"),
      if: state.hasTest && state.selectionHasLinks)
    add(
      .saveToLibrary(.template), String(localized: "Save as Template…"),
      String(localized: "Keeps the selected blocks in the library, to add them to other tests."),
      symbol: ProjectLibrary.templateSymbol, keywords: String(localized: "template block library"),
      if: state.hasTest && state.hasPickedBlocks)
    add(
      .saveToLibrary(.block), String(localized: "Make Block…"),
      String(localized: "Turns the selected blocks into one custom block other tests can run."),
      symbol: ProjectLibrary.defaultBlockSymbol, keywords: String(localized: "custom block library subflow reuse"),
      if: state.hasTest && state.hasPickedBlocks)
    add(
      .arrange, String(localized: "Arrange Automatically"), String(localized: "Lays the blocks out neatly again."),
      symbol: "rectangle.3.group", keywords: String(localized: "layout tidy"), if: state.hasTest)
    add(
      .newTest, String(localized: "New Test…"), String(localized: "Creates an empty test or one to record."),
      symbol: "plus.rectangle.on.rectangle", shortcut: Shortcuts.newTest, if: state.hasProject)
    add(
      .describeTest, String(localized: "Describe a Test…"),
      state.hasTest
        ? String(localized: "The AI rewrites the open test from what you describe in words.")
        : String(localized: "The AI writes a new test from what you describe in words."),
      symbol: "sparkles", keywords: String(localized: "ai write generate draft words text"), if: state.hasProject)
    add(
      .runAll, String(localized: "Run All Tests"), String(localized: "Runs every test of the project in turn."),
      symbol: "play.square.stack", shortcut: Shortcuts.runAll, if: state.hasProject && state.hasTests)
    add(
      .commit, String(localized: "Commit Changes…"), String(localized: "Saves a version of the changed tests in Git."),
      symbol: "checkmark.circle", shortcut: Shortcuts.commit, keywords: String(localized: "git save version"),
      if: git.isTracked && git.hasChanges)
    add(
      .newPullRequest, String(localized: "New Pull Request…"),
      String(localized: "Proposes your branch’s changes on GitHub."), symbol: "arrow.triangle.pull",
      shortcut: Shortcuts.newPullRequest, keywords: String(localized: "git github review propose"),
      if: git.onGitHub && git.hasCommits && !git.isBusy)
    add(
      .pullRequests, String(localized: "Pull Requests"), String(localized: "Shows the pull requests on GitHub."),
      symbol: "list.bullet.rectangle", keywords: String(localized: "git github review"), if: git.onGitHub)
    add(
      .pull, String(localized: "Pull"), String(localized: "Gets the latest commits from the server."),
      symbol: "arrow.down.circle", keywords: String(localized: "git update download"),
      if: git.hasUpstream && !git.isBusy)
    add(
      .push, String(localized: "Push"), String(localized: "Sends your commits to the server."),
      symbol: "arrow.up.circle", keywords: String(localized: "git upload send"),
      if: git.hasRemote && git.hasCommits && !git.isBusy)
    let onBoard = state.boardInView != nil
    add(
      .addToBoard(.test), String(localized: "Add Test to Board…"), String(localized: "Puts a test of the project on the board."),
      symbol: "flowchart", keywords: String(localized: "board card"), if: onBoard && state.boardLacksTests)
    add(
      .addToBoard(.note), String(localized: "Add Note to Board"), String(localized: "Writes a note on the board."),
      symbol: "note.text", keywords: String(localized: "board text markdown comment"), if: onBoard)
    add(
      .addToBoard(.frame), String(localized: "Add Frame to Board"),
      String(localized: "Draws a frame that groups what is inside it."), symbol: "rectangle.dashed",
      keywords: String(localized: "board group section"), if: onBoard)
    add(
      .addToBoard(.link), String(localized: "Add Link to Board…"), String(localized: "Puts a web address on the board."),
      symbol: "globe", keywords: String(localized: "board url web"), if: onBoard)
    add(
      .addToBoard(.image), String(localized: "Add Image to Board…"),
      String(localized: "Puts a picture from your Mac on the board."), symbol: "photo",
      keywords: String(localized: "board picture photo screenshot"), if: onBoard)
    add(
      .addToBoard(.board), String(localized: "Add Link to New Board"),
      String(localized: "Makes a new board and puts a way to it on this one."), symbol: "rectangle.on.rectangle",
      keywords: String(localized: "board"), if: onBoard)
    add(
      .newBoard, String(localized: "New Board"), String(localized: "Makes an empty board to lay tests and notes out on."),
      symbol: "plus.rectangle.on.rectangle", keywords: String(localized: "board canvas map"), if: state.hasProject)
    add(
      .projectStatistics, String(localized: "Project Statistics"),
      String(localized: "The tests that fail most often and the slowest ones."), symbol: "chart.bar.doc.horizontal",
      keywords: String(localized: "stats history trend flaky duration slow"), if: state.hasProject && state.hasRuns)
    add(
      .schedules, String(localized: "Schedules…"), String(localized: "Runs tests by themselves at set times."),
      symbol: "calendar.badge.clock", keywords: String(localized: "timer nightly"), if: state.hasProject)
    add(
      .openProject, String(localized: "Open Project…"), String(localized: "Chooses the folder that holds your tests."),
      symbol: "folder", shortcut: Shortcuts.openProject)
    add(
      .settings, String(localized: "Settings…"), String(localized: "Permissions, AI, Git, iPhone and more."),
      symbol: "gearshape", shortcut: Shortcuts.settings, keywords: String(localized: "preferences options"))
    return list
  }

  private static func blocks(_ state: PaletteState) -> [PaletteEntry] {
    guard state.hasTest else { return [] }
    return BlockInfo.catalog.map { spec in
      let group = BlockInfo.groupTitle(spec.group)
      return PaletteEntry(
        action: .addBlock(spec.typeName), kind: .block, title: BlockInfo.title(spec.typeName),
        detail: BlockInfo.tagline(spec.typeName), symbol: BlockInfo.symbol(spec.typeName),
        color: BlockInfo.color(spec.group), shortcut: nil,
        keywords: [group, BlockInfo.explanation(spec.typeName), String(localized: "add block"), spec.typeName]
          .joined(separator: " "))
    }
  }

  private static func boards(_ state: PaletteState) -> [PaletteEntry] {
    let boards = state.boards.map { name in
      PaletteEntry(
        action: .showBoard(name), kind: .board, title: name, detail: String(localized: "Board"),
        symbol: "rectangle.on.rectangle", color: .secondary, shortcut: nil,
        keywords: String(localized: "board map project open"))
    }
    let items = state.boardItems.map { item in
      PaletteEntry(
        action: .showOnBoard(item.id), kind: .board, title: item.title, detail: item.kind, symbol: "scope",
        color: .secondary, shortcut: nil, keywords: [item.kind, String(localized: "board find show")].joined(separator: " "))
    }
    return boards + items
  }

  private static func library(_ state: PaletteState) -> [PaletteEntry] {
    guard state.hasTest else { return [] }
    return state.library.map { entry in
      let choice = PaletteChoice.library(entry)
      return PaletteEntry(
        action: .addFromLibrary(entry), kind: .block, title: entry.name, detail: choice.tagline, symbol: entry.symbol,
        color: choice.color, shortcut: nil,
        keywords: [choice.groupTitle, entry.detail, String(localized: "template block library")].joined(separator: " "))
    }
  }

  private static func tests(_ state: PaletteState) -> [PaletteEntry] {
    state.tests.map { test in
      PaletteEntry(
        action: .openTest(test.id), kind: .test, title: test.name, detail: test.folder, symbol: "doc.text",
        color: .secondary, shortcut: nil, keywords: [test.folder, String(localized: "open test"), test.keywords].joined(separator: " "))
    }
  }
}
