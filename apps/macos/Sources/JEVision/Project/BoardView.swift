import AppKit
import JEVisionCore
import SwiftUI
import UniformTypeIdentifiers

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
  @AppStorage(GridSnap.key) private var snaps = true
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
  /// How the board looks from afar and how its tests stand, worked out when either changes.
  @State private var look = BoardLook()
  /// Counts the changes of `look`, so the minimap draws again only then.
  @State private var lookRevision = 0
  /// The tests whose blocks show on the board, and the block whose fields are open.
  @State private var documents = BoardTestDocuments()
  @State private var picked: Set<String> = []
  @State private var pickedArrow: String?
  /// The node an arrow is being made from with the keyboard or VoiceOver.
  @State private var connecting: String?
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
      .task(id: RouteKey(generation: boards.generation, map: map)) { await route() }
      .onChange(of: boards.current, initial: true) { old, new in switched(from: old, to: new) }
      .onChange(of: boards.request) { handle(boards.request) }
      .onDisappear { if let name = boards.current { boards.viewports[name] = viewport } }
      .onChange(of: boards.generation, initial: true) { refreshLook() }
      .onChange(of: names) { refreshLook() }
      .onChange(of: map) { refreshLook() }
      .onChange(of: project.latestRuns) { refreshLook() }
      .onChange(of: runningFile) { refreshLook() }
      .onChange(of: waitingFiles) { refreshLook() }
      .onChange(of: project.items) { documents.reload() }
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
    if boards.board != nil {
      canvas(boards.scene)
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
    let tests = look.tests
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

  /// How many nodes VoiceOver is told of at once: a board zoomed out over thousands would swamp it.
  private static let spokenLimit = 150

  private func canvas(_ scene: BoardScene) -> some View {
    let scale = viewport.scale
    let area = visibleRect.insetBy(dx: -60 / scale, dy: -60 / scale)
    let seen = scene.visible(in: area, moved: movedIDs, by: moveShift)
    let laid = seen.nodes.map { scene.layout.nodes[$0] }
    let detailed = scale >= Self.detailZoom
    return ZStack(alignment: .topLeading) {
      BoardGrid(viewport: viewport)
      BoardArrows(
        arrows: seen.edges.compactMap { arrow(at: $0, in: scene) }, uses: shownUses(scene), picked: pickedArrow,
        viewport: viewport, area: area)
      if detailed {
        ForEach(laid, id: \.id) { node in
          if let found = scene.nodes[node.id] { placed(found, rect: shown(node)) }
        }
      } else {
        overview(laid, scene: scene)
      }
      handles(scene, detailed: detailed)
      connection(scene)
      noteEditor(scene)
    }
    // Exactly the space it is given: the nodes are moved into place by offsets, so a node bigger
    // than the view, as an open test can be, must not make the board, and the window, bigger.
    .frame(minWidth: 0, maxWidth: .infinity, minHeight: 0, maxHeight: .infinity, alignment: .topLeading)
    .contentShape(.rect)
    .coordinateSpace(.named(Self.space))
    .gesture(dragGesture(scene))
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
    .overlay(alignment: .bottomTrailing) { minimap(scene).padding(12) }
    .overlay(alignment: .top) { connectionBanner }
    .onGeometryChange(for: CGSize.self) { $0.size } action: {
      size = $0
      fitIfNeeded()
    }
    .clipped()
    .focusable()
    .focused($focus, equals: .board)
    .focusEffectDisabled()
    .onDeleteCommand { removePicked() }
    .onKeyPress(phases: [.down, .repeat]) { press in key(press) }
    .onDrop(of: [.fileURL, .image], isTargeted: nil) { providers, location in
      drop(providers, at: onBoard(location))
    }
    .onPasteCommand(of: [.fileURL, .image]) { providers in
      _ = drop(providers, at: onBoard(CGPoint(x: size.width / 2, y: size.height / 2)))
    }
    .popover(item: inspection, attachmentAnchor: .rect(.rect(inspectionRect(scene))), arrowEdge: .trailing) { inspection in
      inspector(inspection)
    }
    .accessibilityElement(children: .contain)
    .accessibilityLabel(Text("Board"))
  }

  private var inspection: Binding<BoardTestDocuments.Inspection?> {
    Binding(get: { documents.inspected }, set: { if $0 == nil { documents.dismiss() } })
  }

  /// All the fields of a block of an open test, as the editor's inspector shows them.
  @ViewBuilder private func inspector(_ inspection: BoardTestDocuments.Inspection) -> some View {
    if let root = project.rootURL {
      let document = documents.document(of: inspection.file, in: root)
      let last = documents.lastRun(of: inspection.file, document: document, runs: project.runs).last
      InspectorView(document: document, id: inspection.block, failure: last?.failure(of: inspection.block))
        .frame(width: 340, height: 480)
    }
  }

  /// Where the block being looked at is in the view.
  private func inspectionRect(_ scene: BoardScene) -> CGRect {
    guard let inspected = documents.inspected, let root = project.rootURL,
      let node = scene.board.nodes.first(where: { $0.open && BoardLook.test($0) == inspected.file }),
      let place = scene.places[node.id]
    else { return .zero }
    let document = documents.document(of: inspected.file, in: root)
    guard let geometry = BoardBlockGeometry(card: shown(scene.layout.nodes[place]), layout: document.layout),
      let block = document.layout.blocks.first(where: { $0.id == inspected.block })
    else { return .zero }
    return screen(geometry.rect(of: block.rect))
  }

  /// The board from afar: one drawing for what is in view, with the name of each test and how it
  /// stands. An open test big enough to read stays a full view, so its blocks show.
  @ViewBuilder private func overview(_ laid: [LaidNode], scene: BoardScene) -> some View {
    let scale = viewport.scale
    let readable = laid.filter { scene.nodes[$0.id]?.open == true && $0.rect.width * scale >= 60 }
    let skip = Set(readable.map(\.id))
    let items = laid.compactMap { node -> BoardOverview.Item? in
      guard !skip.contains(node.id), let place = scene.places[node.id], place < look.glances.count else { return nil }
      return BoardOverview.Item(id: node.id, rect: shown(node), glance: look.glances[place])
    }
    BoardOverview(items: items, picked: picked, viewport: viewport)
      .accessibilityChildren {
        ZStack(alignment: .topLeading) {
          ForEach(spoken(items), id: \.id) { item in
            if let node = scene.nodes[item.id] {
              let rect = screen(item.rect)
              Color.clear
                .frame(width: rect.width, height: rect.height)
                .modifier(accessibility(node))
                .offset(x: rect.minX, y: rect.minY)
            }
          }
        }
      }
    ForEach(readable, id: \.id) { node in
      if let found = scene.nodes[node.id] { placed(found, rect: shown(node)) }
    }
  }

  /// The items VoiceOver is told of: all of them, or the nearest to the middle of the view.
  private func spoken(_ items: [BoardOverview.Item]) -> [BoardOverview.Item] {
    guard items.count > Self.spokenLimit else { return items }
    let middle = CGPoint(x: visibleRect.midX, y: visibleRect.midY)
    return Array(
      items.sorted { hypot($0.rect.midX - middle.x, $0.rect.midY - middle.y) < hypot($1.rect.midX - middle.x, $1.rect.midY - middle.y) }
        .prefix(Self.spokenLimit))
  }

  /// A node as a full view, drawn at board size and scaled into place. Clicks and drags go to
  /// the board, which knows what lies under the pointer.
  private func placed(_ node: BoardNode, rect: CGRect) -> some View {
    let scale = viewport.scale
    return nodeView(node, rect: rect)
      .frame(width: rect.width, height: rect.height)
      .scaleEffect(scale, anchor: .topLeading)
      .frame(width: rect.width * scale, height: rect.height * scale, alignment: .topLeading)
      .allowsHitTesting(false)
      .modifier(accessibility(node))
      .offset(x: rect.minX * scale + viewport.offset.x, y: rect.minY * scale + viewport.offset.y)
  }

  @ViewBuilder private func nodeView(_ node: BoardNode, rect: CGRect) -> some View {
    let tint = BoardColor.color(node.color)
    let isPicked = picked.contains(node.id)
    switch node.kind {
    case .group(let label, let collapsed, _):
      BoardFrameView(
        label: label, color: tint, isFolded: collapsed, isPicked: isPicked,
        tests: collapsed ? testsInside(node).count : 0, rollup: look.rollups[node.id])
    case .text(let text):
      BoardNoteView(text: editing == node.id ? "" : text, color: tint, isPicked: isPicked)
    case .link(let url):
      BoardLinkCard(
        symbol: "globe", title: URL(string: url)?.host() ?? url, detail: url, color: tint, isPicked: isPicked,
        isMissing: false)
    case .file(let file):
      if ProjectScanner.isTestFile(file) {
        if node.open, let open = openTest(file, size: rect.size, isPicked: isPicked) {
          open
        } else {
          testCard(file, isPicked: isPicked)
        }
      } else if let name = ProjectBoards.name(of: file) {
        BoardLinkCard(
          symbol: "rectangle.on.rectangle", title: name, detail: String(localized: "Board"), color: tint,
          isPicked: isPicked, isMissing: !boards.names.contains(name))
      } else if boards.scene.images.contains(node.id) {
        BoardImageView(file: file, root: project.rootURL, color: tint, isPicked: isPicked)
      } else {
        BoardLinkCard(
          symbol: "doc", title: (file as NSString).lastPathComponent, detail: file, color: tint, isPicked: isPicked,
          isMissing: !fileExists(file))
      }
    }
  }

  private func testCard(_ file: String, isPicked: Bool) -> TestCard {
    let standing = look.status[file]?.kind
    return TestCard(
      name: names[file] ?? ProjectScanner.testName((file as NSString).lastPathComponent), info: map[file],
      last: project.latestRuns[file], quarantine: project.quarantine(of: file), flaky: project.flaky[file],
      uses: (map[file]?.uses ?? []).compactMap { names[$0] }, usedBy: (usedBy[file] ?? []).compactMap { names[$0] },
      needs: (map[file]?.dependsOn ?? []).map { names[$0] ?? $0 },
      isRunning: standing == .running, isMissing: names[file] == nil, isPicked: isPicked,
      isWaiting: standing == .waiting, blocksChevron: hasBlocks(file) ? "chevron.right" : nil)
  }

  /// A test card that shows the blocks inside it; `nil` when the test can't be read.
  private func openTest(_ file: String, size: CGSize, isPicked: Bool) -> BoardOpenTestView? {
    guard let root = project.rootURL, names[file] != nil else { return nil }
    let document = documents.document(of: file, in: root)
    guard document.flow != nil else { return nil }
    let fit = BoardBlockGeometry(card: CGRect(origin: .zero, size: size), layout: document.layout)?.fit ?? 1
    return BoardOpenTestView(
      file: file, name: names[file] ?? file, status: look.status[file] ?? source.status(of: file), document: document,
      documents: documents, run: blockRun(of: file, document: document), size: size,
      detail: BoardBlockDetail(scale: viewport.scale * fit), isPicked: isPicked)
  }

  /// How the blocks of an open test did: in the run going on, else in the last saved one.
  private func blockRun(of file: String, document: FlowDocument) -> BoardBlockRun {
    if let live = runs.marks(for: document) { return BoardBlockRun(marks: live.marks, current: live.current) }
    return documents.lastRun(of: file, document: document, runs: project.runs).run
  }

  /// The test has blocks to show on its card.
  private func hasBlocks(_ file: String) -> Bool {
    names[file] != nil && (map[file]?.blocks ?? 0) > 0
  }

  /// What VoiceOver says of a node and what it can do with it.
  private func accessibility(_ node: BoardNode) -> BoardAccessibility {
    let (label, value) = describe(node)
    let files = testFiles(of: node)
    return BoardAccessibility(
      label: label, value: value, isPicked: picked.contains(node.id), holdsBlocks: node.open && BoardLook.test(node) != nil,
      open: { open(node) }, select: { select(node.id) },
      remove: isRemovable(node) ? { remove([node.id]) } : nil, connect: connectAction(node),
      run: canRun && !files.isEmpty ? { run(files, title: label) } : nil,
      blocks: blocksAction(node))
  }

  private func blocksAction(_ node: BoardNode) -> (title: String, action: () -> Void)? {
    guard let file = BoardLook.test(node), node.open || hasBlocks(file) else { return nil }
    return (node.open ? String(localized: "Hide Blocks") : String(localized: "Show Blocks"), { toggleBlocks(node) })
  }

  private func describe(_ node: BoardNode) -> (String, String) {
    switch node.kind {
    case .group(let label, let collapsed, _):
      let tests = testsInside(node).count
      let count = collapsed ? String(localized: "Folded, \(tests) tests") : String(localized: "\(tests) tests")
      return (String(localized: "Frame \(label)"), [count, look.rollups[node.id]?.text ?? ""].filter { !$0.isEmpty }.joined(separator: ". "))
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
      if boards.scene.images.contains(node.id) { return (String(localized: "Picture"), (file as NSString).lastPathComponent) }
      return (String(localized: "File"), file)
    }
  }

  private func select(_ id: String) {
    picked = [id]
    pickedArrow = nil
  }

  // MARK: Handles

  /// For the one node picked: a corner to resize it by and a dot to draw an arrow from.
  @ViewBuilder private func handles(_ scene: BoardScene, detailed: Bool) -> some View {
    if detailed, picked.count == 1, let id = picked.first, let node = scene.nodes[id], let place = scene.places[id],
      !isFolded(node)
    {
      let laid = scene.layout.nodes[place]
      let original = laid.rect.cgRect
      let rect = screen(shown(laid))
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
              guard let board = boards.board, let target = hit(onBoard(value.location), wholeFrames: true),
                target != id, let edit = connectOnBoard(board: board, from: id, to: target)
              else { return }
              apply(edit.board, action: String(localized: "Add Arrow"))
              picked = []
              pickedArrow = edit.id
            })
    }
  }

  /// The arrow being drawn from a node to the pointer.
  @ViewBuilder private func connection(_ scene: BoardScene) -> some View {
    if case .connect(let from, let to)? = drag, let place = scene.places[from] {
      let start = screen(scene.layout.nodes[place].rect.cgRect)
      Path { path in
        path.move(to: CGPoint(x: start.maxX, y: start.midY))
        path.addLine(to: to)
      }
      .stroke(Color.accentColor, style: StrokeStyle(lineWidth: 2, dash: [6, 4]))
      .allowsHitTesting(false)
    } else if let from = connecting, let source = scene.places[from], let target = picked.first, target != from,
      let end = scene.places[target]
    {
      let start = screen(scene.layout.nodes[source].rect.cgRect)
      let stop = screen(scene.layout.nodes[end].rect.cgRect)
      Path { path in
        path.move(to: CGPoint(x: start.midX, y: start.midY))
        path.addLine(to: CGPoint(x: stop.midX, y: stop.midY))
      }
      .stroke(Color.accentColor, style: StrokeStyle(lineWidth: 2, dash: [6, 4]))
      .allowsHitTesting(false)
    }
  }

  /// Says what the keyboard does while an arrow is being made.
  @ViewBuilder private var connectionBanner: some View {
    if let from = connecting, let node = boards.scene.nodes[from] {
      HStack(spacing: 10) {
        Image(systemName: "arrow.right")
        Text("Pick what to connect “\(describe(node).0)” to, then press Return")
        Button("Cancel") { connecting = nil }
      }
      .padding(.horizontal, 14)
      .padding(.vertical, 8)
      .adaptiveGlass(in: .capsule)
      .padding(.top, 12)
      .accessibilityElement(children: .combine)
    }
  }

  @ViewBuilder private func noteEditor(_ scene: BoardScene) -> some View {
    if let editing, let found = scene.places[editing] {
      let place = screen(scene.layout.nodes[found].rect.cgRect)
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

  @ViewBuilder private func minimap(_ scene: BoardScene) -> some View {
    if showsMinimap {
      BoardMinimap(layout: scene.layout, glances: look.glances, revision: lookRevision, visible: visibleRect) {
        center(on: $0)
      }
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

  private func dragGesture(_ scene: BoardScene) -> some Gesture {
    DragGesture(minimumDistance: 3, coordinateSpace: .named(Self.space))
      .onChanged { value in
        if drag == nil {
          if isInEditor(value.startLocation) { return }
          if let id = hit(onBoard(value.startLocation), wholeFrames: false) {
            if !picked.contains(id) { picked = [id] }
            pickedArrow = nil
            drag = .move(ids: Set(boardWithContents(board: scene.board, ids: Array(picked))), by: .zero)
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
    if connecting != nil {
      if let id = hit(point, wholeFrames: true) { finishConnection(to: id) } else { connecting = nil }
      return
    }
    let scene = boards.scene
    guard let id = hit(point, wholeFrames: false), let node = scene.nodes[id] else {
      picked = []
      pickedArrow = hitArrow(point)
      return
    }
    pickedArrow = nil
    if case .group = node.kind, point.x <= node.rect.x + 44 {
      fold(node)
      return
    }
    if isBlocksChevron(point, of: node) {
      select(id)
      toggleBlocks(node)
      return
    }
    if let file = BoardLook.test(node), let block = block(at: point, of: node) {
      select(id)
      documents.inspect(file, block: block)
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
    if let id = hit(point, wholeFrames: false), let node = boards.scene.nodes[id] {
      if case .group = node.kind, point.x <= node.rect.x + 44 { return }
      if isBlocksChevron(point, of: node) { return }
      if let file = BoardLook.test(node), let block = block(at: point, of: node) {
        project.showBlock(block, in: file)
        return
      }
      open(node)
    } else if hitArrow(point) == nil {
      add(.note, at: point, view: location)
    }
  }

  /// The corner of a test's card where the chevron that shows or hides its blocks is.
  private func isBlocksChevron(_ point: CGPoint, of node: BoardNode) -> Bool {
    guard let file = BoardLook.test(node), node.open || hasBlocks(file) else { return false }
    let rect = node.rect.cgRect
    return point.x >= rect.maxX - 44 && point.y <= rect.minY + 44
  }

  /// The block of an open test at a board point.
  private func block(at point: CGPoint, of node: BoardNode) -> String? {
    guard node.open, let file = BoardLook.test(node), let root = project.rootURL, names[file] != nil else { return nil }
    let document = documents.document(of: file, in: root)
    return BoardBlockGeometry(card: node.rect.cgRect, layout: document.layout)?
      .block(at: point, in: document.layout)
  }

  /// Clicks and drags in the note being written are the text editor's.
  private func isInEditor(_ location: CGPoint) -> Bool {
    let scene = boards.scene
    guard let editing, let place = scene.places[editing] else { return false }
    return screen(scene.layout.nodes[place].rect.cgRect).contains(location)
  }

  /// The topmost node at a board point. A frame counts only by its title bar, so a drag inside
  /// it pans, unless `wholeFrames`.
  private func hit(_ point: CGPoint, wholeFrames: Bool) -> String? {
    boards.scene.hit(point, header: max(boardFrameHeader(), 24 / viewport.scale), wholeFrames: wholeFrames)
  }

  private func hitArrow(_ point: CGPoint) -> String? {
    let scene = boards.scene
    let reach = 8 / viewport.scale
    let near = CGRect(x: point.x - reach, y: point.y - reach, width: 2 * reach, height: 2 * reach)
    return scene.index.visible(area: Rect(near)).edges.lazy.compactMap { arrow(at: Int($0), in: scene) }.first { arrow in
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

  // MARK: Keyboard

  /// Arrow keys go to the nearest node that way, Return opens it, C starts an arrow from it and
  /// Return ends it, R runs its tests, B shows or hides the blocks of a test, Esc lets go.
  private func key(_ press: KeyPress) -> KeyPress.Result {
    guard editing == nil, press.modifiers.isDisjoint(with: [.command, .control, .option]) else { return .ignored }
    switch press.key {
    case .upArrow: navigate(.up)
    case .downArrow: navigate(.down)
    case .leftArrow: navigate(.left)
    case .rightArrow: navigate(.right)
    case .return:
      guard press.phase == .down else { return .handled }
      if connecting != nil {
        finishConnection()
      } else if picked.count == 1, let id = picked.first, let node = boards.scene.nodes[id] {
        open(node)
      } else {
        return .ignored
      }
    case .escape:
      if connecting != nil {
        connecting = nil
      } else {
        picked = []
        pickedArrow = nil
      }
    default:
      guard press.phase == .down else { return .ignored }
      switch press.characters.lowercased() {
      case "c": startConnecting()
      case "r": runPicked()
      case "b": togglePickedBlocks()
      default: return .ignored
      }
    }
    return .handled
  }

  /// Picks the node next to the picked one in `heading`; with none picked, the one nearest the middle.
  private func navigate(_ heading: BoardHeading) {
    let scene = boards.scene
    let target =
      picked.count == 1 && picked.first != nil
      ? scene.neighbour(of: picked.first ?? "", heading)
      : scene.nearest(to: CGPoint(x: visibleRect.midX, y: visibleRect.midY), in: visibleRect)
    guard let target, let place = scene.places[target] else { return NSSound.beep() }
    select(target)
    bringIntoView(scene.layout.nodes[place].rect.cgRect)
  }

  /// Moves the view just enough that `rect` is in it.
  private func bringIntoView(_ rect: CGRect) {
    guard !visibleRect.insetBy(dx: 24 / viewport.scale, dy: 24 / viewport.scale).contains(rect) else { return }
    center(on: CGPoint(x: rect.midX, y: rect.midY))
  }

  private func startConnecting(_ id: String? = nil) {
    guard let id = id ?? (picked.count == 1 ? picked.first : nil) else { return NSSound.beep() }
    connecting = id
    select(id)
    focus = .board
  }

  /// Draws the arrow from the node it was started on to `target`, or to the picked node.
  private func finishConnection(to target: String? = nil) {
    guard let from = connecting else { return }
    connecting = nil
    guard let target = target ?? (picked.count == 1 ? picked.first : nil), target != from else { return }
    guard let board = boards.board, let edit = connectOnBoard(board: board, from: from, to: target) else {
      return NSSound.beep()
    }
    apply(edit.board, action: String(localized: "Add Arrow"))
    picked = []
    pickedArrow = edit.id
  }

  /// What VoiceOver offers for making an arrow: start one, end it here, or give it up.
  private func connectAction(_ node: BoardNode) -> (title: String, action: () -> Void)? {
    guard let from = connecting else { return (String(localized: "Connect"), { startConnecting(node.id) }) }
    if from == node.id { return (String(localized: "Cancel Connecting"), { connecting = nil }) }
    return (String(localized: "Connect Here"), { finishConnection(to: node.id) })
  }

  /// The test files a node stands for: its own test, or the tests inside a frame.
  private func testFiles(of node: BoardNode) -> [String] {
    if let file = BoardLook.test(node) { return names[file] != nil ? [file] : [] }
    if case .group = node.kind { return testsInside(node) }
    return []
  }

  private func runPicked() {
    let scene = boards.scene
    var seen: Set<String> = []
    let files = picked.compactMap { id in scene.places[id].map { ($0, id) } }.sorted { $0.0 < $1.0 }
      .compactMap { scene.nodes[$0.1] }.flatMap(testFiles(of:)).filter { seen.insert($0).inserted }
    guard canRun, let first = files.first else { return NSSound.beep() }
    run(files, title: files.count == 1 ? names[first] ?? first : String(localized: "Selected tests"))
  }

  private func togglePickedBlocks() {
    guard picked.count == 1, let id = picked.first, let node = boards.scene.nodes[id], let file = BoardLook.test(node),
      node.open || hasBlocks(file)
    else { return NSSound.beep() }
    toggleBlocks(node)
  }

  private func toggleBlocks(_ node: BoardNode) {
    do {
      try boards.toggleBlocks(of: node.id, undoManager: undoManager)
    } catch {
      failure = ProjectBoards.message(error)
    }
  }

  // MARK: Pictures

  /// Puts the pictures a drop or a paste holds on the board, side by side from `point`.
  private func drop(_ providers: [NSItemProvider], at point: CGPoint) -> Bool {
    let accepted = providers.filter {
      $0.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) || $0.hasItemConformingToTypeIdentifier(UTType.image.identifier)
    }
    guard !accepted.isEmpty else { return false }
    Task {
      for (index, provider) in accepted.enumerated() {
        guard let picture = await BoardImages.read(provider) else { continue }
        addPicture(picture.data, named: picture.name, at: CGPoint(x: point.x + Double(index) * 32, y: point.y + Double(index) * 32))
      }
    }
    return true
  }

  private func choosePictures(at point: CGPoint) {
    let panel = NSOpenPanel()
    panel.allowedContentTypes = [.image]
    panel.allowsMultipleSelection = true
    panel.message = String(localized: "Choose the pictures to put on the board")
    guard panel.runModal() == .OK else { return }
    for (index, url) in panel.urls.enumerated() {
      do {
        addPicture(
          try Data(contentsOf: url), named: url.lastPathComponent,
          at: CGPoint(x: point.x + Double(index) * 32, y: point.y + Double(index) * 32))
      } catch {
        failure = error.localizedDescription
      }
    }
  }

  private func addPicture(_ data: Data, named name: String, at point: CGPoint) {
    do {
      if let id = try boards.addImage(data, named: name, at: point, undoManager: undoManager) { select(id) }
    } catch {
      failure = ProjectBoards.message(error)
    }
  }

  // MARK: Menus

  private func menu(at location: CGPoint) -> NSMenu? {
    guard let board = boards.board else { return nil }
    let point = onBoard(location)
    let menu = NSMenu()
    menu.autoenablesItems = false
    if let id = hit(point, wholeFrames: false), let node = node(id) {
      if !picked.contains(id) { picked = [id] }
      pickedArrow = nil
      nodeItems(node).forEach(menu.addItem)
      if let connect = connectAction(node) {
        menu.addItem(BoardMenuItem(connect.title, symbol: "arrow.right") { connect.action() })
      }
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
    items.append(BoardMenuItem(String(localized: "Add Image…"), symbol: "photo") { add(.image, at: point, view: view) })
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
        ] + (blocksAction(node).map { blocks in
          [BoardMenuItem(blocks.title, symbol: "square.stack.3d.up") { blocks.action() }]
        } ?? [])
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
    case .image:
      choosePictures(at: point)
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
    let scene = boards.scene
    guard let target = scene.nodes[id] else { return }
    if scene.places[id] == nil {
      var board = scene.board
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
    select(id)
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
    connecting = nil
    documents.dismiss()
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

  /// Where a shown node is, which differs from the layout while it is dragged or resized.
  private func shown(_ node: LaidNode) -> CGRect {
    let rect = node.rect.cgRect
    switch drag {
    case .move(let ids, let by)? where ids.contains(node.id):
      return rect.offsetBy(dx: by.width / viewport.scale, dy: by.height / viewport.scale)
    case .resize(let id, let new)? where id == node.id:
      return new
    default:
      return rect
    }
  }

  /// The nodes being dragged, and how far in board points.
  private var movedIDs: Set<String> {
    if case .move(let ids, _)? = drag { ids } else { [] }
  }

  private var moveShift: CGSize {
    guard case .move(_, let by)? = drag else { return .zero }
    return CGSize(width: by.width / viewport.scale, height: by.height / viewport.scale)
  }

  /// The arrow at `position` in the layout, its ends moved along with the nodes being dragged.
  private func arrow(at position: Int, in scene: BoardScene) -> BoardArrows.Arrow? {
    guard position < scene.layout.edges.count else { return nil }
    let laid = scene.layout.edges[position]
    guard let edge = scene.edges[laid.id] else { return nil }
    let moved = movedIDs
    let by = moveShift
    func shift(_ point: Point, _ node: String) -> CGPoint {
      let point = CGPoint(x: point.x, y: point.y)
      return moved.contains(node) ? CGPoint(x: point.x + by.width, y: point.y + by.height) : point
    }
    return BoardArrows.Arrow(
      id: laid.id, from: shift(laid.from, edge.from), fromSide: laid.fromSide, to: shift(laid.to, edge.to),
      toSide: laid.toSide, fromHead: edge.fromArrow, toHead: edge.toArrow, label: edge.label,
      color: BoardColor.color(edge.color))
  }

  /// The arrows of tests running others, but not from or to cards being dragged.
  private func shownUses(_ scene: BoardScene) -> [ProjectMap.Link] {
    let moved = movedIDs
    guard !moved.isEmpty else { return uses }
    let rects = Set(moved.compactMap { id in scene.places[id].map { scene.layout.nodes[$0].rect.cgRect } })
    return uses.filter { !rects.contains($0.from) && !rects.contains($0.to) }
  }

  private func node(_ id: String) -> BoardNode? {
    boards.scene.nodes[id]
  }

  private func isFolded(_ node: BoardNode) -> Bool {
    if case .group(_, true, _) = node.kind { true } else { false }
  }

  private func frameTitle(_ node: BoardNode) -> String {
    if case .group(let label, _, _) = node.kind { label } else { "" }
  }

  /// The tests whose cards lie inside the frame `frame`, in board order.
  private func testsInside(_ frame: BoardNode) -> [String] {
    (boards.scene.frameTests[frame.id] ?? []).filter { names[$0] != nil }
  }

  /// Tests of the project not yet on the board, for Add Test.
  private var testsOffBoard: [ProjectItem] {
    let on = Set(look.tests)
    return project.items.flatMap(\.tests).filter { !on.contains($0.id) }
  }

  /// The test started now, relative to the project.
  private var startedFile: String? {
    guard let running = runs.running, let root = project.rootURL else { return nil }
    return RelativePath.from(root, to: running.document.url)
  }

  /// The test running now; one waiting for another run to give up the screen hasn't begun.
  private var runningFile: String? {
    runs.state?.isWaitingForScreen == true ? nil : startedFile
  }

  /// The tests of the batch running now that haven't started, and the test waiting for the screen.
  private var waitingFiles: Set<String> {
    var files = Set(
      (runs.batch?.entries ?? []).compactMap { entry in
        if case .waiting = entry.state { entry.file } else { nil }
      })
    if runs.state?.isWaitingForScreen == true, let file = startedFile { files.insert(file) }
    return files
  }

  private var source: BoardStatusSource {
    BoardStatusSource(
      names: names, map: map, latest: project.latestRuns, running: runningFile, waiting: waitingFiles)
  }

  /// Works out again how each test stands and how the board looks from afar.
  private func refreshLook() {
    let scene = boards.scene
    look = BoardLook(scene, source)
    lookRevision += 1
    documents.keep(only: Set(scene.board.nodes.compactMap { $0.open ? BoardLook.test($0) : nil }))
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
    let generation: Int
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

/// VoiceOver's view of a board node: what it is, what it says, and what can be done with it,
/// since clicks on the board go to the board itself.
private struct BoardAccessibility: ViewModifier {
  let label: String
  let value: String
  let isPicked: Bool
  /// An open test: its blocks are elements of their own inside it.
  let holdsBlocks: Bool
  let open: () -> Void
  let select: () -> Void
  let remove: (() -> Void)?
  let connect: (title: String, action: () -> Void)?
  let run: (() -> Void)?
  let blocks: (title: String, action: () -> Void)?

  func body(content: Content) -> some View {
    content
      .accessibilityElement(children: holdsBlocks ? .contain : .ignore)
      .accessibilityLabel(Text(verbatim: label))
      .accessibilityValue(Text(verbatim: value))
      .accessibilityAddTraits(isPicked ? [.isButton, .isSelected] : .isButton)
      .accessibilityHint(Text("Opens it"))
      .accessibilityAction { open() }
      .accessibilityAction(named: Text("Select")) { select() }
      .accessibilityActions {
        if let connect { Button(action: connect.action) { Text(verbatim: connect.title) } }
        if let run { Button("Run", action: run) }
        if let blocks { Button(action: blocks.action) { Text(verbatim: blocks.title) } }
        if let remove { Button("Remove from Board", action: remove) }
      }
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
