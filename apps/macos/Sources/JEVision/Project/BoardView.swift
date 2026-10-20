import AppKit
import JEVisionCore
import SwiftUI

/// The board in view: an endless plane with the project's tests, frames, notes, links and other
/// boards on it. Two fingers or the wheel pan, a pinch or ⌘ with the wheel zooms, a drag moves
/// what it starts on or else the board, a double click opens what is under it or writes a note,
/// and a right click adds things or changes the one under the pointer.
struct BoardView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(RunCenter.self) private var runs
  @Environment(RecordCenter.self) private var records
  @Environment(\.undoManager) private var undoManager
  @AppStorage("boardShowsMinimap") private var showsMinimap = true
  @AppStorage("boardSnapsToGrid") private var snaps = true
  @State private var viewport = BoardViewport()
  @State private var size = CGSize.zero
  /// The board hasn't been looked at yet: once its size is known, it zooms to show everything.
  @State private var needsFit = true
  /// What the core read from each test, by file relative to the project.
  @State private var map: [String: MapTest] = [:]
  /// The names of the project's tests, by file relative to the project.
  @State private var names: [String: String] = [:]
  /// The tests that run each test, by file.
  @State private var usedBy: [String: [String]] = [:]
  /// Dashed arrows from each test card to the cards of the tests it runs, in board points.
  @State private var uses: [ProjectMap.Link] = []
  @State private var picked: Set<String> = []
  @State private var pickedArrow: String?
  @State private var drag: Drag?
  /// The note being written.
  @State private var editing: String?
  @State private var noteText = ""
  @State private var testPicker: Place?
  @State private var prompt: Prompt?
  @State private var promptText = ""
  @State private var failure: String?
  @FocusState private var focus: Focus?

  static let zoomRange = 0.05...2.0
  /// From this zoom on, nodes are full views; further out, one drawing shows them all.
  static let detailZoom = 0.4
  private static let space = "board"

  private enum Focus { case board, note }

  private enum Drag {
    case pan(from: CGPoint)
    case move(ids: Set<String>, by: CGSize)
    case resize(id: String, rect: CGRect)
    case connect(from: String, to: CGPoint)
  }

  /// A point of the board and where it shows in the view.
  struct Place: Identifiable {
    let id = UUID()
    let board: CGPoint
    let view: CGPoint
  }

  private enum Prompt: Identifiable {
    case newLink(at: CGPoint)
    case editLink(String)
    case frameTitle(String)

    var id: String {
      switch self {
      case .newLink: "new link"
      case .editLink(let id): "link " + id
      case .frameTitle(let id): "frame " + id
      }
    }
  }

  private var boards: ProjectBoards { project.boards }
  private var isAllTests: Bool { boards.current != nil && boards.current == boards.allTests }
  private var canRun: Bool { !runs.isRunning && !records.isRecording && project.rootURL != nil }

  var body: some View {
    content
      .safeAreaInset(edge: .top, spacing: 0) { header }
      .task(id: project.items) { await loadMap() }
      .task(id: RouteKey(board: boards.board, map: map)) { await route() }
      .onChange(of: boards.current, initial: true) { old, new in switched(from: old, to: new) }
      .onChange(of: boards.request) { handle(boards.request) }
      .onDisappear { if let name = boards.current { boards.viewports[name] = viewport } }
      .popover(item: $testPicker, attachmentAnchor: .point(pickerAnchor), arrowEdge: .bottom) { place in
        BoardTestPicker(tests: testsOffBoard) { test in
          testPicker = nil
          insert(.file(file: test), at: place.board, action: String(localized: "Add Test to Board"))
        }
      }
      .alert(promptTitle, isPresented: Binding(get: { prompt != nil }, set: { if !$0 { prompt = nil } }), presenting: prompt) { prompt in
        TextField(promptField(prompt), text: $promptText)
        Button("OK") { answer(prompt) }
        Button("Cancel", role: .cancel) {}
      }
      .alert(
        "Something went wrong", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } }),
        presenting: failure
      ) { _ in
        Button("OK") {}
      } message: { Text($0) }
      .focusedSceneValue(\.canvasZoom, canvasZoom)
  }

  @ViewBuilder private var content: some View {
    if let board = boards.board {
      canvas(board, layout: boards.layout)
        .unmirrored()
    } else if let problem = boards.problem {
      ContentUnavailableView {
        Label("Can’t Show This Board", systemImage: "exclamationmark.triangle")
      } description: {
        Text(problem)
      }
    } else {
      ContentUnavailableView(
        "No Boards Yet", systemImage: "rectangle.on.rectangle",
        description: Text("A board of all tests appears once the project has a test. Make your own with the + button next to Boards."))
    }
  }

  // MARK: Header

  private var header: some View {
    let tests = testsOnBoard
    return HStack(spacing: 16) {
      VStack(alignment: .leading, spacing: 2) {
        Text(verbatim: boards.current ?? "")
          .font(.title2.weight(.semibold))
          .lineLimit(1)
        Text("\(tests.count) tests")
          .font(.callout)
          .foregroundStyle(.secondary)
      }
      Spacer(minLength: 12)
      HStack(spacing: 6) {
        DashedLine()
          .stroke(.secondary, style: StrokeStyle(lineWidth: 1.5, dash: [5, 4]))
          .frame(width: 28, height: 2)
        Text("runs another test")
        DashedLine()
          .stroke(.tint, style: StrokeStyle(lineWidth: 1.5))
          .frame(width: 28, height: 2)
          .padding(.leading, 8)
        Text("needs it to pass first")
      }
      .font(.callout)
      .foregroundStyle(.secondary)
      .accessibilityHidden(true)
      Toggle(isOn: $snaps) {
        Label("Snap to Grid", systemImage: "square.grid.3x3")
      }
      .toggleStyle(.button)
      .labelStyle(.iconOnly)
      .help(Text("Snap what you move to the grid; hold ⌘ while moving to do the opposite"))
      Menu {
        RunModePicker(folder: ".") { failure = $0 }
      } label: {
        Label("Run All", systemImage: "play.fill")
      } primaryAction: {
        guard let root = project.rootURL else { return }
        let title = isAllTests ? String(localized: "All tests") : boards.current ?? ""
        runs.runAll(tests.map { root.appending(path: $0) }, in: root, title: title)
      }
      .menuStyle(.button)
      .buttonStyle(.glassProminent)
      .fixedSize()
      .help(Text("Run every test on this board, one after another or side by side as chosen in the menu"))
      .accessibilityHint(Text("Runs every test. Open the menu to choose whether they run one after another or side by side."))
      .disabled(!canRun || tests.isEmpty)
    }
    .padding(.horizontal, 24)
    .padding(.vertical, 14)
  }

  // MARK: Canvas

  private func canvas(_ board: Board, layout: BoardLayout) -> some View {
    let nodes = Dictionary(board.nodes.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
    let rects = shownRects(layout)
    let area = CGRect(origin: .zero, size: size).insetBy(dx: -60, dy: -60)
    let visible = rects.filter { screen($0.rect).intersects(area) }
    let glances = glances(board)
    let detailed = viewport.scale >= Self.detailZoom
    return ZStack(alignment: .topLeading) {
      BoardGrid(viewport: viewport)
      BoardArrows(arrows: arrows(board, layout: layout), uses: shownUses, picked: pickedArrow, viewport: viewport)
      if detailed {
        ForEach(visible, id: \.id) { item in
          if let node = nodes[item.id] { placed(node, rect: item.rect) }
        }
      } else {
        BoardOverview(nodes: visible, glances: glances, picked: picked, viewport: viewport)
          .accessibilityChildren {
            ZStack(alignment: .topLeading) {
              ForEach(visible, id: \.id) { item in
                if let node = nodes[item.id] {
                  let rect = screen(item.rect)
                  Color.clear
                    .frame(width: rect.width, height: rect.height)
                    .modifier(accessibility(node))
                    .offset(x: rect.minX, y: rect.minY)
                }
              }
            }
          }
      }
      handles(nodes: nodes, layout: layout, detailed: detailed)
      connection(layout)
      noteEditor(layout)
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    .contentShape(.rect)
    .coordinateSpace(.named(Self.space))
    .gesture(dragGesture(board, layout: layout))
    .simultaneousGesture(SpatialTapGesture(count: 2, coordinateSpace: .named(Self.space)).onEnded { doubleClick($0.location) })
    .simultaneousGesture(SpatialTapGesture(coordinateSpace: .named(Self.space)).onEnded { click($0.location) })
    .overlay {
      BoardInput(
        pan: { by in viewport.offset = CGPoint(x: viewport.offset.x + by.width, y: viewport.offset.y + by.height) },
        zoom: { factor, at in zoom(to: viewport.scale * factor, around: at) },
        smartZoom: { at in if viewport.scale < 0.95 { zoom(to: 1, around: at) } else { fit() } },
        menu: { menu(at: $0) })
    }
    .overlay(alignment: .bottomLeading) {
      ZoomBar(zoom: canvasZoom, fitHelp: Text("Show everything on the board (⌘0)"))
        .padding(12)
    }
    .overlay(alignment: .bottomTrailing) { minimap(layout: layout, glances: glances).padding(12) }
    .onGeometryChange(for: CGSize.self) { $0.size } action: {
      size = $0
      fitIfNeeded()
    }
    .clipped()
    .focusable()
    .focused($focus, equals: .board)
    .focusEffectDisabled()
    .onDeleteCommand { removePicked() }
    .onKeyPress(.escape) {
      picked = []
      pickedArrow = nil
      return .handled
    }
    .accessibilityElement(children: .contain)
    .accessibilityLabel(Text("Board"))
  }

  /// A node as a full view, drawn at board size and scaled into place. Clicks and drags go to
  /// the board, which knows what lies under the pointer.
  private func placed(_ node: BoardNode, rect: CGRect) -> some View {
    let scale = viewport.scale
    return nodeView(node)
      .frame(width: rect.width, height: rect.height)
      .scaleEffect(scale, anchor: .topLeading)
      .frame(width: rect.width * scale, height: rect.height * scale, alignment: .topLeading)
      .allowsHitTesting(false)
      .modifier(accessibility(node))
      .offset(x: rect.minX * scale + viewport.offset.x, y: rect.minY * scale + viewport.offset.y)
  }

  @ViewBuilder private func nodeView(_ node: BoardNode) -> some View {
    let tint = BoardColor.color(node.color)
    let isPicked = picked.contains(node.id)
    switch node.kind {
    case .group(let label, let collapsed, _):
      BoardFrameView(
        label: label, color: tint, isFolded: collapsed, isPicked: isPicked,
        tests: collapsed ? testsInside(node).count : 0)
    case .text(let text):
      BoardNoteView(text: editing == node.id ? "" : text, color: tint, isPicked: isPicked)
    case .link(let url):
      BoardLinkCard(
        symbol: "globe", title: URL(string: url)?.host() ?? url, detail: url, color: tint, isPicked: isPicked,
        isMissing: false)
    case .file(let file):
      if ProjectScanner.isTestFile(file) {
        testCard(file, isPicked: isPicked)
      } else if let name = ProjectBoards.name(of: file) {
        BoardLinkCard(
          symbol: "rectangle.on.rectangle", title: name, detail: String(localized: "Board"), color: tint,
          isPicked: isPicked, isMissing: !boards.names.contains(name))
      } else {
        BoardLinkCard(
          symbol: "doc", title: (file as NSString).lastPathComponent, detail: file, color: tint, isPicked: isPicked,
          isMissing: !fileExists(file))
      }
    }
  }

  private func testCard(_ file: String, isPicked: Bool) -> TestCard {
    TestCard(
      name: names[file] ?? ProjectScanner.testName((file as NSString).lastPathComponent), info: map[file],
      last: project.latestRuns[file], quarantine: project.quarantine(of: file), flaky: project.flaky[file],
      uses: (map[file]?.uses ?? []).compactMap { names[$0] }, usedBy: (usedBy[file] ?? []).compactMap { names[$0] },
      needs: (map[file]?.dependsOn ?? []).map { names[$0] ?? $0 },
      isRunning: isRunning(file), isMissing: names[file] == nil, isPicked: isPicked)
  }

  /// What VoiceOver says of a node and what it can do with it.
  private func accessibility(_ node: BoardNode) -> BoardAccessibility {
    let (label, value) = describe(node)
    return BoardAccessibility(
      label: label, value: value, isPicked: picked.contains(node.id),
      open: { open(node) }, select: { picked = [node.id] },
      remove: isRemovable(node) ? { remove([node.id]) } : nil)
  }

  private func describe(_ node: BoardNode) -> (String, String) {
    switch node.kind {
    case .group(let label, let collapsed, _):
      let tests = testsInside(node).count
      return (
        String(localized: "Frame \(label)"),
        collapsed ? String(localized: "Folded, \(tests) tests") : String(localized: "\(tests) tests"))
    case .text(let text):
      return (String(localized: "Note"), text.isEmpty ? String(localized: "Empty") : text)
    case .link(let url):
      return (String(localized: "Link"), url)
    case .file(let file):
      if ProjectScanner.isTestFile(file) {
        let card = testCard(file, isPicked: false)
        return (card.name, card.accessibilityValue)
      }
      if let name = ProjectBoards.name(of: file) { return (String(localized: "Board \(name)"), "") }
      return (String(localized: "File"), file)
    }
  }

  // MARK: Handles

  /// For the one node picked: a corner to resize it by and a dot to draw an arrow from.
  @ViewBuilder private func handles(nodes: [String: BoardNode], layout: BoardLayout, detailed: Bool) -> some View {
    if detailed, picked.count == 1, let id = picked.first, let node = nodes[id],
      let laid = layout.nodes.first(where: { $0.id == id }), !isFolded(node)
    {
      let original = laid.rect.cgRect
      let shown = shownRects(layout).first { $0.id == id }?.rect ?? original
      let rect = screen(shown)
      BoardHandle(kind: .resize)
        .position(x: rect.maxX, y: rect.maxY)
        .gesture(
          DragGesture(coordinateSpace: .named(Self.space))
            .onChanged { value in
              let scale = viewport.scale
              drag = .resize(
                id: id,
                rect: CGRect(
                  x: original.minX, y: original.minY, width: max(48, original.width + value.translation.width / scale),
                  height: max(48, original.height + value.translation.height / scale)))
            }
            .onEnded { _ in
              defer { drag = nil }
              guard case .resize(_, let new)? = drag, let board = boards.board, new != original else { return }
              apply(resizeOnBoard(board: board, id: id, rect: Rect(new)), action: String(localized: "Resize"))
            })
      BoardHandle(kind: .connect)
        .position(x: rect.maxX + 16, y: rect.midY)
        .gesture(
          DragGesture(coordinateSpace: .named(Self.space))
            .onChanged { drag = .connect(from: id, to: $0.location) }
            .onEnded { value in
              drag = nil
              guard let board = boards.board, let target = hit(onBoard(value.location), layout: layout, wholeFrames: true),
                target != id, let edit = connectOnBoard(board: board, from: id, to: target)
              else { return }
              apply(edit.board, action: String(localized: "Add Arrow"))
              picked = []
              pickedArrow = edit.id
            })
    }
  }

  /// The arrow being drawn from a node to the pointer.
  @ViewBuilder private func connection(_ layout: BoardLayout) -> some View {
    if case .connect(let from, let to)? = drag, let rect = layout.nodes.first(where: { $0.id == from })?.rect.cgRect {
      let start = screen(rect)
      Path { path in
        path.move(to: CGPoint(x: start.maxX, y: start.midY))
        path.addLine(to: to)
      }
      .stroke(Color.accentColor, style: StrokeStyle(lineWidth: 2, dash: [6, 4]))
      .allowsHitTesting(false)
    }
  }

  @ViewBuilder private func noteEditor(_ layout: BoardLayout) -> some View {
    if let editing, let rect = layout.nodes.first(where: { $0.id == editing })?.rect.cgRect {
      let place = screen(rect)
      TextEditor(text: $noteText)
        .font(.system(size: NSFont.systemFontSize * viewport.scale))
        .scrollContentBackground(.hidden)
        .padding(10 * viewport.scale)
        .frame(width: place.width, height: place.height)
        .background(.background, in: .rect(cornerRadius: 14 * viewport.scale))
        .overlay(RoundedRectangle(cornerRadius: 14 * viewport.scale).strokeBorder(.tint, lineWidth: 2))
        .offset(x: place.minX, y: place.minY)
        .focused($focus, equals: .note)
        .onAppear { focus = .note }
        .onChange(of: focus) { if focus != .note { endEditing() } }
        .onKeyPress(.escape) {
          endEditing()
          return .handled
        }
        .accessibilityLabel(Text("Note"))
    }
  }

  // MARK: Minimap

  @ViewBuilder private func minimap(layout: BoardLayout, glances: [String: BoardGlance]) -> some View {
    if showsMinimap {
      BoardMinimap(layout: layout, glances: glances, visible: visibleRect) { center(on: $0) }
        .padding(6)
        .adaptiveGlass(in: .rect(cornerRadius: 16))
        .overlay(alignment: .topTrailing) {
          Button("Hide Minimap", systemImage: "xmark") { showsMinimap = false }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
            .controlSize(.small)
            .padding(8)
            .help(Text("Hide the minimap"))
        }
    } else {
      Button("Show Minimap", systemImage: "map") { showsMinimap = true }
        .labelStyle(.iconOnly)
        .buttonStyle(.borderless)
        .padding(8)
        .adaptiveGlass(in: .circle)
        .help(Text("Show the whole board small in the corner"))
    }
  }

  // MARK: Pointer

  private func dragGesture(_ board: Board, layout: BoardLayout) -> some Gesture {
    DragGesture(minimumDistance: 3, coordinateSpace: .named(Self.space))
      .onChanged { value in
        if drag == nil {
          if isInEditor(value.startLocation) { return }
          if let id = hit(onBoard(value.startLocation), layout: layout, wholeFrames: false) {
            if !picked.contains(id) { picked = [id] }
            pickedArrow = nil
            drag = .move(ids: Set(boardWithContents(board: board, ids: Array(picked))), by: .zero)
          } else {
            drag = .pan(from: viewport.offset)
          }
        }
        switch drag {
        case .pan(let from)?:
          viewport.offset = CGPoint(x: from.x + value.translation.width, y: from.y + value.translation.height)
        case .move(let ids, _)?:
          drag = .move(ids: ids, by: value.translation)
        default: break
        }
      }
      .onEnded { value in
        defer { drag = nil }
        guard case .move? = drag, let board = boards.board else { return }
        let by = Point(x: value.translation.width / viewport.scale, y: value.translation.height / viewport.scale)
        let snap = snaps != NSEvent.modifierFlags.contains(.command)
        apply(moveOnBoard(board: board, ids: Array(picked), by: by, snap: snap), action: String(localized: "Move"))
      }
  }

  private func click(_ location: CGPoint) {
    if isInEditor(location) { return }
    if editing != nil { endEditing() }
    focus = .board
    let point = onBoard(location)
    let layout = boards.layout
    guard let id = hit(point, layout: layout, wholeFrames: false), let node = node(id) else {
      picked = []
      pickedArrow = hitArrow(point)
      return
    }
    pickedArrow = nil
    if case .group = node.kind, point.x <= node.rect.x + 44 {
      fold(node)
      return
    }
    let flags = NSEvent.modifierFlags
    if flags.contains(.shift) || flags.contains(.command) {
      if picked.contains(id) { picked.remove(id) } else { picked.insert(id) }
    } else {
      picked = [id]
    }
  }

  private func doubleClick(_ location: CGPoint) {
    if isInEditor(location) { return }
    let point = onBoard(location)
    if let id = hit(point, layout: boards.layout, wholeFrames: false), let node = node(id) {
      if case .group = node.kind, point.x <= node.rect.x + 44 { return }
      open(node)
    } else if hitArrow(point) == nil {
      add(.note, at: point, view: location)
    }
  }

  /// Clicks and drags in the note being written are the text editor's.
  private func isInEditor(_ location: CGPoint) -> Bool {
    guard let editing, let rect = boards.layout.nodes.first(where: { $0.id == editing })?.rect.cgRect else { return false }
    return screen(rect).contains(location)
  }

  /// The topmost node at a board point. A frame counts only by its title bar, so a drag inside
  /// it pans, unless `wholeFrames`.
  private func hit(_ point: CGPoint, layout: BoardLayout, wholeFrames: Bool) -> String? {
    let header = max(boardFrameHeader(), 24 / viewport.scale)
    for laid in layout.nodes.reversed() {
      let rect = laid.rect.cgRect
      guard rect.contains(point) else { continue }
      if !wholeFrames, case .group? = node(laid.id)?.kind, point.y > rect.minY + header { continue }
      return laid.id
    }
    return nil
  }

  private func hitArrow(_ point: CGPoint) -> String? {
    guard let board = boards.board else { return nil }
    let reach = 8 / viewport.scale
    return arrows(board, layout: boards.layout).first { arrow in
      let curve = BoardArrows.curve(arrow, scale: viewport.scale)
      let points = (0...32).map { curve.point(at: Double($0) / 32) }
      return zip(points, points.dropFirst()).contains { Self.distance(point, $0, $1) < reach }
    }?.id
  }

  private static func distance(_ p: CGPoint, _ a: CGPoint, _ b: CGPoint) -> Double {
    let (dx, dy) = (b.x - a.x, b.y - a.y)
    let length = dx * dx + dy * dy
    let t = length == 0 ? 0 : min(max(((p.x - a.x) * dx + (p.y - a.y) * dy) / length, 0), 1)
    return hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy))
  }

  // MARK: Menus

  private func menu(at location: CGPoint) -> NSMenu? {
    guard let board = boards.board else { return nil }
    let point = onBoard(location)
    let menu = NSMenu()
    menu.autoenablesItems = false
    if let id = hit(point, layout: boards.layout, wholeFrames: false), let node = node(id) {
      if !picked.contains(id) { picked = [id] }
      pickedArrow = nil
      nodeItems(node).forEach(menu.addItem)
      menu.addItem(colorMenu { color in recolor(nodes: picked, color: color) })
      menu.addItem(.separator())
      let removable = board.nodes.filter { picked.contains($0.id) && isRemovable($0) }.map(\.id)
      let title = picked.count > 1 ? String(localized: "Remove \(picked.count) Items from Board") : String(localized: "Remove from Board")
      menu.addItem(BoardMenuItem(title, symbol: "trash", enabled: !removable.isEmpty) { remove(removable) })
    } else if let arrow = hitArrow(point) {
      picked = []
      pickedArrow = arrow
      menu.addItem(colorMenu { color in recolor(arrow: arrow, color: color) })
      menu.addItem(BoardMenuItem(String(localized: "Remove Arrow"), symbol: "trash") { removeArrow(arrow) })
    } else {
      picked = []
      pickedArrow = nil
      addItems(at: point, view: location).forEach(menu.addItem)
      menu.addItem(.separator())
      menu.addItem(BoardMenuItem(String(localized: "Show Everything"), symbol: "arrow.up.left.and.arrow.down.right") { fit() })
      menu.addItem(BoardMenuItem(String(localized: "Snap to Grid"), checked: snaps) { snaps.toggle() })
      menu.addItem(BoardMenuItem(String(localized: "Show Minimap"), checked: showsMinimap) { showsMinimap.toggle() })
    }
    return menu
  }

  private func addItems(at point: CGPoint, view: CGPoint) -> [NSMenuItem] {
    var items: [NSMenuItem] = []
    if !testsOffBoard.isEmpty {
      items.append(BoardMenuItem(String(localized: "Add Test…"), symbol: "flowchart") { add(.test, at: point, view: view) })
    }
    items.append(BoardMenuItem(String(localized: "Add Note"), symbol: "note.text") { add(.note, at: point, view: view) })
    items.append(BoardMenuItem(String(localized: "Add Frame"), symbol: "rectangle.dashed") { add(.frame, at: point, view: view) })
    items.append(BoardMenuItem(String(localized: "Add Link…"), symbol: "globe") { add(.link, at: point, view: view) })
    let others = boards.names.filter { $0 != boards.current }
    let links = others.map { name in
      BoardMenuItem(name, symbol: "rectangle.on.rectangle") {
        insert(.file(file: ProjectBoards.path(of: name)), at: point, action: String(localized: "Add Board Link"))
      }
    }
    let new = BoardMenuItem(String(localized: "New Board…"), symbol: "plus") { add(.board, at: point, view: view) }
    items.append(BoardMenuItem.submenu(
      String(localized: "Add Link to Board"), symbol: "rectangle.on.rectangle",
      items: links + (links.isEmpty ? [] : [.separator()]) + [new]))
    return items
  }

  private func nodeItems(_ node: BoardNode) -> [NSMenuItem] {
    switch node.kind {
    case .group(_, let collapsed, _):
      let tests = testsInside(node)
      return [
        BoardMenuItem(String(localized: "Rename Frame…"), symbol: "pencil") { open(node) },
        BoardMenuItem(collapsed ? String(localized: "Unfold") : String(localized: "Fold"), symbol: collapsed ? "chevron.down" : "chevron.forward") { fold(node) },
        BoardMenuItem(String(localized: "Run Tests in Frame"), symbol: "play.fill", enabled: canRun && !tests.isEmpty) { run(tests, title: frameTitle(node)) },
      ]
    case .text:
      return [BoardMenuItem(String(localized: "Edit Note"), symbol: "pencil") { open(node) }]
    case .link:
      return [
        BoardMenuItem(String(localized: "Open Link"), symbol: "safari") { open(node) },
        BoardMenuItem(String(localized: "Edit Link…"), symbol: "pencil") { editLink(node) },
      ]
    case .file(let file):
      if ProjectScanner.isTestFile(file) {
        let exists = names[file] != nil
        return [
          BoardMenuItem(String(localized: "Open Test"), symbol: "flowchart", enabled: exists) { open(node) },
          BoardMenuItem(String(localized: "Run"), symbol: "play.fill", enabled: exists && canRun) {
            run([file], title: names[file] ?? file)
          },
        ]
      }
      if let name = ProjectBoards.name(of: file) {
        return [BoardMenuItem(String(localized: "Open Board"), symbol: "rectangle.on.rectangle", enabled: boards.names.contains(name)) { open(node) }]
      }
      return [
        BoardMenuItem(String(localized: "Open"), symbol: "doc", enabled: fileExists(file)) { open(node) },
        BoardMenuItem(String(localized: "Show in Finder"), symbol: "folder", enabled: fileExists(file)) {
          if let root = project.rootURL { NSWorkspace.shared.activateFileViewerSelecting([root.appending(path: file)]) }
        },
      ]
    }
  }

  private func colorMenu(_ choose: @escaping (String) -> Void) -> NSMenuItem {
    let none = BoardMenuItem(String(localized: "No Color"), image: BoardMenuItem.swatch(nil)) { choose("") }
    let presets = BoardColor.presets.map { preset in
      BoardMenuItem(preset.name, image: BoardMenuItem.swatch(preset.color)) { choose(preset.id) }
    }
    return BoardMenuItem.submenu(String(localized: "Color"), symbol: "paintpalette", items: [none, .separator()] + presets)
  }

  // MARK: Actions

  private func add(_ addition: BoardAddition, at point: CGPoint, view: CGPoint) {
    switch addition {
    case .test:
      testPicker = Place(board: point, view: view)
    case .note:
      insert(.text(text: ""), at: point, action: String(localized: "Add Note")) { id in
        noteText = ""
        editing = id
      }
    case .frame:
      insert(.group(label: String(localized: "Frame"), collapsed: false, folder: nil), at: point, action: String(localized: "Add Frame")) { id in
        promptText = String(localized: "Frame")
        prompt = .frameTitle(id)
      }
    case .link:
      promptText = ""
      prompt = .newLink(at: point)
    case .board:
      do {
        let current = boards.current
        let name = try boards.create(String(localized: "New Board"))
        if let current { boards.select(current) }
        insert(.file(file: ProjectBoards.path(of: name)), at: point, action: String(localized: "Add Board Link"))
        boards.renaming = name
      } catch {
        failure = ProjectBoards.message(error)
      }
    }
  }

  private func insert(_ kind: BoardNodeKind, at point: CGPoint, action: String, then: (String) -> Void = { _ in }) {
    guard let board = boards.board else { return }
    let edit = addToBoard(board: board, kind: kind, at: Point(x: point.x, y: point.y))
    apply(edit.board, action: action)
    picked = [edit.id]
    pickedArrow = nil
    then(edit.id)
  }

  private func open(_ node: BoardNode) {
    switch node.kind {
    case .group(let label, _, _):
      promptText = label
      prompt = .frameTitle(node.id)
    case .text(let text):
      noteText = text
      picked = [node.id]
      editing = node.id
    case .link(let url):
      if let url = URL(string: url) { NSWorkspace.shared.open(url) }
    case .file(let file):
      if ProjectScanner.isTestFile(file) {
        guard names[file] != nil else { return NSSound.beep() }
        project.selection = file
      } else if let name = ProjectBoards.name(of: file) {
        guard boards.names.contains(name) else { return NSSound.beep() }
        project.showBoard(name)
      } else if let root = project.rootURL, fileExists(file) {
        NSWorkspace.shared.open(root.appending(path: file))
      } else {
        NSSound.beep()
      }
    }
  }

  private func editLink(_ node: BoardNode) {
    guard case .link(let url) = node.kind else { return }
    promptText = url
    prompt = .editLink(node.id)
  }

  private func endEditing() {
    guard let id = editing else { return }
    editing = nil
    focus = .board
    change(id, action: String(localized: "Edit Note")) { node in
      if case .text = node.kind { node.kind = .text(text: noteText) }
    }
  }

  private func fold(_ node: BoardNode) {
    guard case .group(let label, let collapsed, let folder) = node.kind else { return }
    change(node.id, action: collapsed ? String(localized: "Unfold Frame") : String(localized: "Fold Frame")) { node in
      node.kind = .group(label: label, collapsed: !collapsed, folder: folder)
    }
  }

  private func recolor(nodes ids: Set<String>, color: String) {
    guard var board = boards.board else { return }
    for i in board.nodes.indices where ids.contains(board.nodes[i].id) { board.nodes[i].color = color }
    apply(board, action: String(localized: "Change Color"))
  }

  private func recolor(arrow id: String, color: String) {
    guard var board = boards.board, let i = board.edges.firstIndex(where: { $0.id == id }) else { return }
    board.edges[i].color = color
    apply(board, action: String(localized: "Change Color"))
  }

  private func change(_ id: String, action: String, _ body: (inout BoardNode) -> Void) {
    guard var board = boards.board, let i = board.nodes.firstIndex(where: { $0.id == id }) else { return }
    body(&board.nodes[i])
    apply(board, action: action)
  }

  private func remove(_ ids: [String]) {
    guard let board = boards.board, !ids.isEmpty else { return }
    apply(removeFromBoard(board: board, ids: ids), action: String(localized: "Remove from Board"))
    picked.subtract(ids)
  }

  private func removeArrow(_ id: String) {
    guard var board = boards.board else { return }
    board.edges.removeAll { $0.id == id }
    apply(board, action: String(localized: "Remove Arrow"))
    pickedArrow = nil
  }

  private func removePicked() {
    if let pickedArrow { return removeArrow(pickedArrow) }
    guard let board = boards.board else { return }
    let ids = board.nodes.filter { picked.contains($0.id) && isRemovable($0) }.map(\.id)
    if ids.isEmpty { NSSound.beep() } else { remove(ids) }
  }

  /// Test cards stay on the board of all tests: they come back as long as the test is there.
  private func isRemovable(_ node: BoardNode) -> Bool {
    guard isAllTests, case .file(let file) = node.kind else { return true }
    return !ProjectScanner.isTestFile(file) || names[file] == nil
  }

  private func apply(_ board: Board, action: String) {
    do {
      try boards.edit(board, action: action, undoManager: undoManager)
    } catch {
      failure = ProjectBoards.message(error)
    }
  }

  private func run(_ tests: [String], title: String) {
    guard let root = project.rootURL else { return }
    runs.runAll(tests.map { root.appending(path: $0) }, in: root, title: title)
  }

  private var promptTitle: Text {
    switch prompt {
    case .newLink?: Text("Add Link")
    case .editLink?: Text("Edit Link")
    case .frameTitle?, nil: Text("Frame Title")
    }
  }

  private func promptField(_ prompt: Prompt) -> LocalizedStringKey {
    switch prompt {
    case .newLink, .editLink: "Web address"
    case .frameTitle: "Title"
    }
  }

  private func answer(_ prompt: Prompt) {
    let text = promptText.trimmingCharacters(in: .whitespacesAndNewlines)
    switch prompt {
    case .frameTitle(let id):
      change(id, action: String(localized: "Rename Frame")) { node in
        if case .group(_, let collapsed, let folder) = node.kind {
          node.kind = .group(label: text, collapsed: collapsed, folder: folder)
        }
      }
    case .newLink(let point):
      guard let url = Self.webAddress(text) else { return failure = String(localized: "“\(text)” isn’t a web address.") }
      insert(.link(url: url), at: point, action: String(localized: "Add Link"))
    case .editLink(let id):
      guard let url = Self.webAddress(text) else { return failure = String(localized: "“\(text)” isn’t a web address.") }
      change(id, action: String(localized: "Edit Link")) { $0.kind = .link(url: url) }
    }
  }

  /// `text` as a web address, with `https://` in front when it names no scheme; `nil` when it
  /// can't be one.
  static func webAddress(_ text: String) -> String? {
    let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
    let full = text.contains("://") ? text : "https://" + text
    guard !text.isEmpty, !text.contains(" "), let url = URL(string: full), url.host()?.isEmpty == false,
      let scheme = url.scheme?.lowercased(), ["http", "https"].contains(scheme)
    else { return nil }
    return full
  }

  // MARK: Palette

  private func handle(_ request: BoardRequest?) {
    guard let request else { return }
    boards.request = nil
    let middle = CGPoint(x: size.width / 2, y: size.height / 2)
    switch request {
    case .add(let addition): add(addition, at: onBoard(middle), view: middle)
    case .reveal(let id): reveal(id)
    case .fit: fit()
    }
  }

  /// Unfolds the frames that hide node `id`, puts it in the middle and picks it.
  private func reveal(_ id: String) {
    guard var board = boards.board, let target = board.nodes.first(where: { $0.id == id }) else { return }
    if !boards.layout.nodes.contains(where: { $0.id == id }) {
      for i in board.nodes.indices {
        if case .group(let label, true, let folder) = board.nodes[i].kind,
          board.nodes[i].rect.cgRect.contains(target.rect.cgRect)
        {
          board.nodes[i].kind = .group(label: label, collapsed: false, folder: folder)
        }
      }
      apply(board, action: String(localized: "Unfold Frame"))
    }
    if viewport.scale < 0.75 { viewport.scale = 1 }
    let rect = target.rect.cgRect
    center(on: CGPoint(x: rect.midX, y: rect.midY))
    picked = [id]
    pickedArrow = nil
  }

  // MARK: Viewport

  private var canvasZoom: CanvasZoom {
    CanvasZoom(
      value: viewport.scale, set: { zoom(to: $0, around: CGPoint(x: size.width / 2, y: size.height / 2)) },
      fit: { fit() }, range: Self.zoomRange)
  }

  private func zoom(to value: Double, around anchor: CGPoint) {
    let new = min(max(value, Self.zoomRange.lowerBound), Self.zoomRange.upperBound)
    guard new != viewport.scale else { return }
    let ratio = new / viewport.scale
    viewport = BoardViewport(
      offset: CGPoint(x: anchor.x - (anchor.x - viewport.offset.x) * ratio, y: anchor.y - (anchor.y - viewport.offset.y) * ratio),
      scale: new)
  }

  /// Zooms so everything on the board is in view; never above actual size.
  private func fit() {
    guard size.width > 0 else { return }
    guard let bounds = boards.layout.bounds?.cgRect, bounds.width > 0, bounds.height > 0 else {
      viewport = BoardViewport(offset: CGPoint(x: size.width / 2, y: size.height / 2), scale: 1)
      return
    }
    let margin = 40.0
    let fits = min((size.width - 2 * margin) / bounds.width, (size.height - 2 * margin) / bounds.height)
    let scale = min(1, max(Self.zoomRange.lowerBound, fits))
    viewport = BoardViewport(
      offset: CGPoint(x: size.width / 2 - bounds.midX * scale, y: size.height / 2 - bounds.midY * scale), scale: scale)
  }

  private func fitIfNeeded() {
    guard needsFit, size.width > 0, boards.board != nil else { return }
    needsFit = false
    fit()
  }

  private func center(on point: CGPoint) {
    viewport.offset = CGPoint(x: size.width / 2 - point.x * viewport.scale, y: size.height / 2 - point.y * viewport.scale)
  }

  private func switched(from old: String?, to new: String?) {
    if let old, old != new { boards.viewports[old] = viewport }
    picked = []
    pickedArrow = nil
    editing = nil
    if let new, let saved = boards.viewports[new] {
      viewport = saved
      needsFit = false
    } else {
      needsFit = true
      fitIfNeeded()
    }
  }

  /// The part of the board in view, in board points.
  private var visibleRect: CGRect {
    CGRect(
      x: -viewport.offset.x / viewport.scale, y: -viewport.offset.y / viewport.scale,
      width: size.width / viewport.scale, height: size.height / viewport.scale)
  }

  private func screen(_ rect: CGRect) -> CGRect {
    CGRect(
      x: rect.minX * viewport.scale + viewport.offset.x, y: rect.minY * viewport.scale + viewport.offset.y,
      width: rect.width * viewport.scale, height: rect.height * viewport.scale)
  }

  private func onBoard(_ point: CGPoint) -> CGPoint {
    CGPoint(x: (point.x - viewport.offset.x) / viewport.scale, y: (point.y - viewport.offset.y) / viewport.scale)
  }

  private var pickerAnchor: UnitPoint {
    guard let place = testPicker, size.width > 0, size.height > 0 else { return .center }
    return UnitPoint(x: place.view.x / size.width, y: place.view.y / size.height)
  }

  // MARK: What is on the board

  /// The shown nodes in drawing order, where they are while something is dragged.
  private func shownRects(_ layout: BoardLayout) -> [(id: String, rect: CGRect)] {
    layout.nodes.map { node in
      var rect = node.rect.cgRect
      switch drag {
      case .move(let ids, let by)? where ids.contains(node.id):
        rect = rect.offsetBy(dx: by.width / viewport.scale, dy: by.height / viewport.scale)
      case .resize(let id, let new)? where id == node.id:
        rect = new
      default: break
      }
      return (node.id, rect)
    }
  }

  private func arrows(_ board: Board, layout: BoardLayout) -> [BoardArrows.Arrow] {
    let edges = Dictionary(board.edges.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
    var moved: Set<String> = []
    if case .move(let ids, _)? = drag { moved = ids }
    let by = { () -> CGSize in
      if case .move(_, let by)? = drag { return CGSize(width: by.width / viewport.scale, height: by.height / viewport.scale) }
      return .zero
    }()
    return layout.edges.compactMap { laid in
      guard let edge = edges[laid.id] else { return nil }
      func shift(_ point: Point, _ node: String) -> CGPoint {
        let point = CGPoint(x: point.x, y: point.y)
        return moved.contains(node) ? CGPoint(x: point.x + by.width, y: point.y + by.height) : point
      }
      return BoardArrows.Arrow(
        id: laid.id, from: shift(laid.from, edge.from), fromSide: laid.fromSide, to: shift(laid.to, edge.to),
        toSide: laid.toSide, fromHead: edge.fromArrow, toHead: edge.toArrow, label: edge.label,
        color: BoardColor.color(edge.color))
    }
  }

  /// The arrows of tests running others, but not from or to cards being dragged.
  private var shownUses: [ProjectMap.Link] {
    guard case .move? = drag else { return uses }
    let moving = shownRects(boards.layout).filter { item in boards.layout.nodes.contains { $0.id == item.id && $0.rect.cgRect != item.rect } }
    let rects = moving.compactMap { item in boards.layout.nodes.first { $0.id == item.id }?.rect.cgRect }
    return uses.filter { link in !rects.contains(link.from) && !rects.contains(link.to) }
  }

  private func glances(_ board: Board) -> [String: BoardGlance] {
    var glances: [String: BoardGlance] = [:]
    let running = runningFile
    for node in board.nodes {
      let tint = BoardColor.color(node.color)
      switch node.kind {
      case .group(let label, _, _):
        glances[node.id] = BoardGlance(shape: .frame, title: label, color: tint)
      case .text(let text):
        glances[node.id] = BoardGlance(shape: .note, title: String(text.prefix { $0 != "\n" }), color: tint)
      case .link(let url):
        glances[node.id] = BoardGlance(shape: .card, title: URL(string: url)?.host() ?? url, color: tint)
      case .file(let file):
        if ProjectScanner.isTestFile(file) {
          let status = running == file ? Color.accentColor : project.latestRuns[file]?.status.color
          let name = names[file] ?? ProjectScanner.testName((file as NSString).lastPathComponent)
          glances[node.id] = BoardGlance(shape: .card, title: name, color: status ?? tint)
        } else {
          glances[node.id] = BoardGlance(shape: .card, title: ProjectBoards.name(of: file) ?? file, color: tint)
        }
      }
    }
    return glances
  }

  private func node(_ id: String) -> BoardNode? {
    boards.board?.nodes.first { $0.id == id }
  }

  private func isFolded(_ node: BoardNode) -> Bool {
    if case .group(_, true, _) = node.kind { true } else { false }
  }

  private func frameTitle(_ node: BoardNode) -> String {
    if case .group(let label, _, _) = node.kind { label } else { "" }
  }

  /// The tests whose cards lie inside the frame `frame`, in board order.
  private func testsInside(_ frame: BoardNode) -> [String] {
    let area = frame.rect.cgRect
    return (boards.board?.nodes ?? []).compactMap { node in
      guard node.id != frame.id, case .file(let file) = node.kind, ProjectScanner.isTestFile(file),
        area.contains(node.rect.cgRect), names[file] != nil
      else { return nil }
      return file
    }
  }

  /// The tests on the board that are in the project, each once, in board order.
  private var testsOnBoard: [String] {
    var seen: Set<String> = []
    return (boards.board?.nodes ?? []).compactMap { node in
      guard case .file(let file) = node.kind, ProjectScanner.isTestFile(file), names[file] != nil,
        seen.insert(file).inserted
      else { return nil }
      return file
    }
  }

  /// Tests of the project not yet on the board, for Add Test.
  private var testsOffBoard: [ProjectItem] {
    let on = Set(testsOnBoard)
    return project.items.flatMap(\.tests).filter { !on.contains($0.id) }
  }

  private func isRunning(_ file: String) -> Bool {
    runningFile == file
  }

  /// The test running now, relative to the project.
  private var runningFile: String? {
    guard let running = runs.running, let root = project.rootURL else { return nil }
    return RelativePath.from(root, to: running.document.url)
  }

  private func fileExists(_ file: String) -> Bool {
    guard let root = project.rootURL else { return false }
    return FileManager.default.fileExists(atPath: root.appending(path: file).path)
  }

  // MARK: Loading

  private func loadMap() async {
    guard let root = project.rootURL else { return }
    let path = root.path(percentEncoded: false)
    let tests = project.items.flatMap(\.tests)
    names = Dictionary(tests.map { ($0.id, $0.name) }, uniquingKeysWith: { a, _ in a })
    let files = tests.map(\.id)
    let read = await Task.detached(priority: .userInitiated) { projectMap(project: path, tests: files) }.value
    guard !Task.isCancelled else { return }
    map = Dictionary(read.map { ($0.file, $0) }, uniquingKeysWith: { a, _ in a })
    usedBy = ProjectMap.usedBy(map)
  }

  private struct RouteKey: Hashable {
    let board: Board?
    let map: [String: MapTest]
  }

  /// Routes the arrows of tests that run other tests around the cards, off the main thread: on
  /// a board of hundreds of tests that takes long enough to stall the window.
  private func route() async {
    guard let board = boards.board else { return uses = [] }
    let layout = boards.layout
    let map = self.map
    let header = boardFrameHeader()
    let links = await Task.detached(priority: .userInitiated) { () -> [ProjectMap.Link] in
      let kinds = Dictionary(board.nodes.map { ($0.id, $0.kind) }, uniquingKeysWith: { a, _ in a })
      var bounds: [String: CGRect] = [:]
      for laid in layout.nodes {
        switch kinds[laid.id] {
        case .file(let file)? where ProjectScanner.isTestFile(file):
          if bounds[file] == nil { bounds[file] = laid.rect.cgRect }
        case .group?:
          let rect = laid.rect.cgRect
          bounds[ProjectMap.labelKey(laid.id)] = CGRect(x: rect.minX, y: rect.minY, width: rect.width, height: min(header, rect.height))
        default: break
        }
      }
      return ProjectMap.links(map, bounds: bounds)
    }.value
    guard !Task.isCancelled else { return }
    uses = links
  }
}

/// VoiceOver's view of a board node: what it is, what it says, and opening, picking and
/// removing it, since clicks on the board go to the board itself.
private struct BoardAccessibility: ViewModifier {
  let label: String
  let value: String
  let isPicked: Bool
  let open: () -> Void
  let select: () -> Void
  let remove: (() -> Void)?

  func body(content: Content) -> some View {
    content
      .accessibilityElement(children: .ignore)
      .accessibilityLabel(Text(verbatim: label))
      .accessibilityValue(Text(verbatim: value))
      .accessibilityAddTraits(isPicked ? [.isButton, .isSelected] : .isButton)
      .accessibilityHint(Text("Opens it"))
      .accessibilityAction { open() }
      .accessibilityAction(named: Text("Select")) { select() }
      .accessibilityAction(named: Text("Remove from Board")) { remove?() }
  }
}

/// A grip on the picked node: its corner resizes it, the dot beside it draws an arrow.
private struct BoardHandle: View {
  enum Kind { case resize, connect }
  let kind: Kind

  var body: some View {
    Group {
      switch kind {
      case .resize:
        RoundedRectangle(cornerRadius: 3)
          .fill(.background)
          .strokeBorder(.tint, lineWidth: 2)
          .frame(width: 12, height: 12)
      case .connect:
        Image(systemName: "arrow.right.circle.fill")
          .font(.system(size: 18))
          .foregroundStyle(.tint)
          .background(.background, in: .circle)
      }
    }
    .padding(6)
    .contentShape(.rect)
    .help(kind == .resize ? Text("Drag to resize") : Text("Drag to another item to draw an arrow"))
    .accessibilityHidden(true)
  }
}

/// The tests to choose from when adding one to a board, with a search field.
struct BoardTestPicker: View {
  let tests: [ProjectItem]
  let choose: (String) -> Void
  @State private var query = ""
  @FocusState private var searching: Bool

  var body: some View {
    let found = query.trimmingCharacters(in: .whitespaces).isEmpty ? tests : ProjectScanner.tests(in: tests, matching: query)
    VStack(spacing: 0) {
      TextField("Search tests", text: $query)
        .textFieldStyle(.roundedBorder)
        .focused($searching)
        .onSubmit { if let first = found.first { choose(first.id) } }
        .padding(10)
      Divider()
      List(found) { test in
        Button { choose(test.id) } label: {
          VStack(alignment: .leading, spacing: 1) {
            Text(verbatim: test.name)
            if !test.parentPath.isEmpty {
              Text(verbatim: test.parentPath)
                .font(.caption)
                .foregroundStyle(.secondary)
            }
          }
          .frame(maxWidth: .infinity, alignment: .leading)
          .contentShape(.rect)
        }
        .buttonStyle(.plain)
      }
      .overlay {
        if found.isEmpty { ContentUnavailableView.search(text: query) }
      }
    }
    .frame(width: 300, height: 340)
    .onAppear { searching = true }
  }
}
