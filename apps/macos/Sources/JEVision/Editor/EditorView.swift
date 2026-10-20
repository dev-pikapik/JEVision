import JEVisionCore
import SwiftUI
import UniformTypeIdentifiers

/// One test: the canvas in the middle, the selected block's inspector on the right.
struct EditorView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(RunCenter.self) private var runs
  @Environment(RecordCenter.self) private var records
  @Environment(\.undoManager) private var undoManager
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  let document: FlowDocument
  @State private var zoom = 1.0
  @State private var pinchStart: Double?
  @State private var showsInspector = true
  @State private var showsDetails = false
  /// Where the palette adds a block; `nil` while it is closed.
  @State private var palette: PaletteTarget?
  /// How far the canvas is scrolled, in zoomed points.
  @State private var scroll = CGPoint.zero
  @State private var scrollPosition = ScrollPosition()
  /// Where to scroll once the canvas has its size at a new zoom.
  @State private var pendingScroll: CGPoint?
  @State private var viewport = CGSize.zero
  /// The pointer over the canvas, in the visible part of it; zooming keeps the spot under it in place.
  @State private var pointer: CGPoint?
  @Environment(\.layoutDirection) private var layoutDirection
  @FocusState private var canvasFocused: Bool
  private let apps = AppCatalog.shared

  var body: some View {
    Group {
      if let failure = document.failure {
        ContentUnavailableView {
          Label("Can’t open this test", systemImage: "exclamationmark.triangle")
        } description: {
          Text(failure)
            .textSelection(.enabled)
            .frame(maxWidth: 480)
        } actions: {
          Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([document.url]) }
        }
      } else if let flow = document.flow {
        editor(flow)
      }
    }
    .onAppear {
      document.undoManager = undoManager
      apps.load()
    }
    .task(id: project.rootURL) {
      document.author = await project.git.authorName()
      document.people = await project.git.authors()
    }
    .confirmationDialog(
      removalTitle, isPresented: Binding(get: { document.pendingRemoval != nil }, set: { if !$0 { document.finishRemoval(notes: nil) } }),
      presenting: document.pendingRemoval
    ) { _ in
      Button("Delete with the Notes", role: .destructive) { document.finishRemoval(notes: .delete) }
      Button("Keep the Notes on the Canvas") { document.finishRemoval(notes: .keep) }
      Button("Cancel", role: .cancel) {}
    } message: { removal in
      Text(removalMessage(removal))
    }
    .onChange(of: undoManager) { document.undoManager = undoManager }
    .sheet(item: Binding(get: { document.savingToLibrary }, set: { document.savingToLibrary = $0 })) { kind in
      SaveToLibrarySheet(kind: kind, document: document)
    }
  }

  private func editor(_ flow: Flow) -> some View {
    let lastRun = project.rootURL.map {
      LastRun(
        file: RelativePath.from($0, to: document.url), flow: flow, runs: project.runs, pinned: document.reportRun)
    }
    return canvas(outcomes: lastRun?.results.mapValues(\.outcome) ?? [:], taken: lastRun?.taken ?? [])
      .unmirrored()
      .overlay(alignment: .topLeading) {
        if let target = palette {
          GeometryReader { box in
            PaletteView(library: project.library.entries) { choice in
              palette = nil
              canvasFocused = true
              switch choice {
              case .block(let type):
                document.insert(type, at: target.slot, placedAt: target.point)
                showsInspector = true
              case .library(let entry):
                document.add(entry, at: target.slot, placedAt: target.point)
                showsInspector = entry.kind == .block
              case .note: addNote(at: target.point)
              case .frame: addFrame(at: target.point)
              case .describe: describeTest()
              }
            } close: {
              palette = nil
              canvasFocused = true
            }
            .environment(\.layoutDirection, layoutDirection)
            .offset(paletteOffset(target, in: box.size))
          }
          .unmirrored()
          .transition(reduceMotion ? .opacity : .scale(scale: 0.96).combined(with: .opacity))
        }
      }
      .overlay(alignment: .top) {
        if let drafter = document.drafter {
          DraftPanel(document: document, drafter: drafter)
            .transition(reduceMotion ? .opacity : .move(edge: .top).combined(with: .opacity))
        }
      }
      .overlay(alignment: .bottom) {
        VStack(spacing: 0) {
          if !document.layout.blocks.isEmpty {
            HStack(spacing: 8) {
              ZoomBar(zoom: canvasZoom)
              if document.layout.blocks.count <= 2 { AddBlockHint(open: openPalette) }
            }
            .padding(.bottom, 12)
          }
          if let result = runs.result, result.test == document.url {
            RunResultBar(
              result: result, showFailedBlock: failedBlock.map { id in { showBlock(id) } },
              addCheck: result.nothingChecked ? { addCheck() } : nil)
              .transition(reduceMotion ? .opacity : .move(edge: .bottom).combined(with: .opacity))
          } else if !document.layout.blocks.isEmpty, document.issues.contains(where: { $0.code == "flow.noCheck" }) {
            HStack(spacing: 10) {
              Image(systemName: RunStatus.unchecked.symbol)
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
              Text("This test never checks the screen, so it can’t catch a wrong result.")
                .fixedSize(horizontal: false, vertical: true)
              Button("Add Check", systemImage: "eye", action: addCheck)
                .buttonStyle(.glassProminent)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
            .adaptiveGlass()
            .padding()
          }
          if let error = document.saveError {
            Label("Changes aren’t saved: \(error)", systemImage: "exclamationmark.triangle.fill")
              .padding(.horizontal, 14)
              .padding(.vertical, 8)
              .adaptiveGlass(tint: .red.opacity(0.3))
              .padding()
          }
        }
      }
      .animation(reduceMotion ? nil : .snappy(duration: 0.2), value: palette)
      .animation(reduceMotion ? nil : .snappy(duration: 0.2), value: document.drafter == nil)
      .animation(reduceMotion ? nil : .snappy, value: runs.result)
      .onChange(of: document.flow) { runs.clearMarks(of: document) }
      .onChange(of: document.saveError) { _, error in
        if let error { AccessibilityNotification.Announcement(String(localized: "Changes aren’t saved: \(error)")).post() }
      }
      .onChange(of: runs.result) { _, result in
        // A new run of the test is what the blocks show from now on.
        if result?.test == document.url { document.reportRun = nil }
      }
      .inspector(isPresented: inspectorShown) {
        Group {
          if let selection = document.selection {
            InspectorView(document: document, id: selection, failure: lastRun?.failure(of: selection))
          } else if let note = document.pickedNote {
            NoteInspector(document: document, id: note.id)
          }
        }
        .inspectorColumnWidth(min: 280, ideal: 320, max: 440)
        // Esc hands the keyboard back to the canvas, where ⌫ deletes the selected block.
        .onExitCommand { canvasFocused = true }
      }
      .toolbar {
        ToolbarItem(placement: .navigation) {
          Button { showsDetails.toggle() } label: {
            HStack(spacing: 4) {
              Text(flow.name.isEmpty ? String(localized: "Untitled Test") : flow.name)
                .font(.headline)
              Image(systemName: "chevron.down").imageScale(.small).foregroundStyle(.secondary)
            }
          }
          .help(Text("Test details"))
          .accessibilityHint(Text("Shows the test card: what it checks, who looks after it, its retries, flaky mark and secrets."))
          .popover(isPresented: $showsDetails, arrowEdge: .bottom) {
            TestDetailsView(document: document)
          }
        }
        ToolbarItemGroup(placement: .primaryAction) {
          AppPicker(
            bundleID: Binding(
              get: { flow.app.isEmpty ? nil : flow.app },
              set: { app in document.updateFlow(field: "app") { $0.app = app ?? "" } }),
            noneTitle: Text("Choose App")
          )
          .help(Text("The app the test runs in. Click to choose another one."))
          LastRunChip(run: lastRun?.own)
        }
        ToolbarSpacer(.fixed, placement: .primaryAction)
        // Run and Record come first: in a narrow window the last items move to the overflow menu,
        // and adding blocks also has Space, ⌘B and ⇧⌘B.
        ToolbarItemGroup(placement: .primaryAction) {
          Menu {
            Button("Run Step by Step", systemImage: "forward.frame") { run(stepByStep: true) }
            if let selection = document.selection {
              Button("Run from Selected Block", systemImage: "arrow.down.to.line") { run(from: selection) }
            }
            runChoices(flow)
          } label: {
            Label("Run", systemImage: "play.fill")
          } primaryAction: {
            run()
          }
          .disabled(runs.isRunning || records.isRecording || project.rootURL == nil || document.beforeDraft != nil)
          .labelStyle(.titleAndIcon)
          .help(Text("Run the test (⌘R)"))
          Button("Record", systemImage: "record.circle") { records.record(document) }
            .disabled(runs.isRunning || records.isRecording || project.rootURL == nil || document.beforeDraft != nil)
            .labelStyle(.titleAndIcon)
            .help(Text("Record steps by using the app; they go after the selected block (⇧⌘R)"))
        }
        if !flow.canvas.notes.isEmpty {
          ToolbarItem(placement: .primaryAction) {
            Menu {
              if let selection = document.selection {
                Button("Add Note to Block", systemImage: "note.text.badge.plus") { document.addNote(about: selection) }
              }
              Toggle(
                "Hide Resolved Notes", systemImage: "eye.slash",
                isOn: Binding(get: { document.hidesResolvedNotes }, set: { document.hidesResolvedNotes = $0 })
              )
              .disabled(!document.hasResolvedNotes && !document.hidesResolvedNotes)
            } label: {
              Label("Notes", systemImage: "text.bubble")
            }
            .help(Text("Notes of the test"))
          }
        }
        ToolbarItemGroup(placement: .primaryAction) {
          Menu {
            Button("Add Note", systemImage: "note.text") { addNote(at: nil) }
            Button("Add Frame", systemImage: "rectangle.dashed") { addFrame(at: nil) }
            Button("Describe the Test…", systemImage: "sparkles") { describeTest() }
            Divider()
            Button("Arrange in Rows", systemImage: "rectangle.grid.3x2") { document.arrangeInRows() }
              .disabled(flow.blocks.isEmpty)
            Button("Arrange Automatically", systemImage: "rectangle.3.group") { document.arrange() }
              .disabled(flow.canvas.positions.isEmpty)
          } label: {
            Label("Add Block", systemImage: "plus")
          } primaryAction: {
            openPalette()
          }
          .labelStyle(.titleAndIcon)
          .help(Text("Add a block after the selected one (Space or ⌘B)"))
          let growsIf = document.selection.flatMap(document.block)?.kind.typeName == "if"
          Button {
            document.addCondition()
            showsInspector = true
          } label: {
            Label {
              growsIf ? Text("Add Condition") : Text("Add If")
            } icon: {
              Image(systemName: "arrow.triangle.branch")
            }
          }
          .help(
            growsIf
              ? Text("Add one more way out to the selected If (⇧⌘B)")
              : Text("Add an If after the selected block, so the test can take different paths (⇧⌘B)"))
        }
      }
      .focusedSceneValue(\.canvasZoom, canvasZoom)
      .focusedSceneValue(\.blockPalette, BlockPaletteOpener { openPalette() })
  }

  private var canvasZoom: CanvasZoom {
    CanvasZoom(value: zoom, set: { setZoom($0) }, fit: fitZoom)
  }

  /// The size of the canvas in the scroll view at zoom `zoom`.
  private func contentSize(at zoom: Double) -> CGSize {
    CGSize(
      width: max(document.layout.width * zoom, viewport.width),
      height: max(document.layout.height * zoom, viewport.height))
  }

  /// Zooms to `value`, keeping the canvas under `anchor`, a point of the visible part, in place;
  /// without one, under the pointer or in the middle.
  private func setZoom(_ value: Double, anchor: CGPoint? = nil) {
    let new = Zoom.clamp(value)
    guard new != zoom else { return }
    let at = anchor ?? pointer ?? CGPoint(x: viewport.width / 2, y: viewport.height / 2)
    let target = Zoom.offset(
      scroll, keeping: at, from: zoom, to: new, content: contentSize(at: new), viewport: viewport)
    let resized = contentSize(at: new) != contentSize(at: zoom)
    zoom = new
    scrollCanvas(to: target, resized: resized)
  }

  /// Zooms so the whole test is in view, centered.
  private func fitZoom() {
    let new = Zoom.fit(CGSize(width: document.layout.width, height: document.layout.height), in: viewport)
    let content = contentSize(at: new)
    let resized = content != contentSize(at: zoom)
    zoom = new
    scrollCanvas(
      to: CGPoint(x: (content.width - viewport.width) / 2, y: (content.height - viewport.height) / 2), resized: resized)
  }

  /// Scrolls to `point`; again once the canvas has its new size, when the zoom just resized it.
  private func scrollCanvas(to point: CGPoint, resized: Bool) {
    pendingScroll = resized ? point : nil
    scrollPosition.scrollTo(point: point)
  }

  /// The environment, data row and retries the next runs use.
  @ViewBuilder private func runChoices(_ flow: Flow) -> some View {
    let document = document
    let environments = project.environments.current
    Divider()
    if !environments.all.isEmpty {
      Picker(
        selection: Binding(
          get: { environments.selected?.name ?? "" },
          set: { project.environments.select($0) })
      ) {
        ForEach(environments.all, id: \.name) { Text(verbatim: $0.name).tag($0.name) }
      } label: {
        Label("Environment: \(environments.selected?.name ?? "")", systemImage: "server.rack")
      }
      .pickerStyle(.menu)
    }
    if let table = RunChoices.dataTable(of: flow, folder: document.url.deletingLastPathComponent()),
      !table.rows.isEmpty
    {
      Picker(selection: Binding(get: { document.runChoices.dataRow }, set: { document.runChoices.dataRow = $0 })) {
        Text("All Rows").tag(Int?.none)
        Divider()
        ForEach(Array(RunChoices.rowTitles(of: table).enumerated()), id: \.offset) { index, title in
          Text(verbatim: title).tag(Int?.some(index))
        }
      } label: {
        Label(RunChoices.dataTitle(row: document.runChoices.dataRow, of: table), systemImage: "tablecells")
      }
      .pickerStyle(.menu)
    }
    Picker(selection: Binding(get: { document.runChoices.retries }, set: { document.runChoices.retries = $0 })) {
      Text("As in Test (\(project.retries(of: flow)))").tag(Int?.none)
      Divider()
      ForEach(RunChoices.retryChoices, id: \.self) { Text($0, format: .number).tag(Int?.some($0)) }
    } label: {
      Label(RunChoices.retriesTitle(document.runChoices.retries, test: project.retries(of: flow)), systemImage: "arrow.clockwise")
    }
    .pickerStyle(.menu)
  }

  private func run(from block: String? = nil, stepByStep: Bool = false) {
    guard let root = project.rootURL else { return }
    runs.run(document, in: root, from: block, stepByStep: stepByStep)
  }

  private var inspectorShown: Binding<Bool> {
    Binding(
      get: { showsInspector && (document.selection != nil || document.pickedNote != nil) },
      set: { showsInspector = $0 })
  }

  private func canvas(outcomes: [String: BlockOutcome], taken: Set<Slot>) -> some View {
    GeometryReader { viewport in
      ScrollView([.horizontal, .vertical]) {
        let visible = CGSize(width: viewport.size.width / zoom, height: viewport.size.height / zoom)
        FlowCanvas(document: document, appName: apps.name(of:), minSize: visible, outcomes: outcomes, taken: taken) { items, block in
          document.pick(items, prefer: block)
          showsInspector = true
          canvasFocused = true
        } onAdd: { slot in
          palette = PaletteTarget(slot: slot)
        } onPalette: { slot, point, at in
          palette = PaletteTarget(
            slot: slot ?? document.slotAfterSelection, point: point,
            anchor: CGPoint(x: at.x * zoom - scroll.x, y: at.y * zoom - scroll.y))
        }
        .scaleEffect(zoom, anchor: .topLeading)
        .frame(
          width: max(document.layout.width, visible.width) * zoom,
          height: max(document.layout.height, visible.height) * zoom, alignment: .topLeading)
        .contentShape(.rect)
        .onTapGesture {
          document.pick([])
          canvasFocused = true
        }
      }
      .scrollPosition($scrollPosition)
      .onScrollGeometryChange(for: CGPoint.self) { $0.contentOffset } action: { scroll = $1 }
      .onScrollGeometryChange(for: CGSize.self) { $0.contentSize } action: { _, _ in
        // A new zoom resizes the canvas after it asked to scroll; scroll again to where it asked.
        if let pendingScroll { scrollPosition.scrollTo(point: pendingScroll) }
        pendingScroll = nil
      }
      .overlay(alignment: .topTrailing) {
        if project.rootURL != nil, let flow = document.flow {
          EnvironmentStrip(flow: flow)
            .padding(12)
            .unmirrored()
        }
      }
      .overlay {
        if document.layout.blocks.isEmpty {
          ContentUnavailableView {
            Label("No blocks yet", systemImage: "square.dashed")
          } description: {
            Text("Add the first step of the test.")
          } actions: {
            Button("Add Block") { openPalette() }
              .buttonStyle(.glassProminent)
          }
        }
      }
    }
    .onGeometryChange(for: CGSize.self) { $0.size } action: { viewport = $0 }
    .onContinuousHover { phase in
      if case .active(let at) = phase { pointer = at } else { pointer = nil }
    }
    .gesture(
      MagnifyGesture()
        .onChanged { value in
          let start = pinchStart ?? zoom
          pinchStart = start
          setZoom(
            start * value.magnification,
            anchor: CGPoint(x: value.startAnchor.x * viewport.width, y: value.startAnchor.y * viewport.height))
        }
        .onEnded { _ in pinchStart = nil }
    )
    .focusable()
    .focusEffectDisabled()
    .focused($canvasFocused)
    .onAppear { canvasFocused = true }
    .onKeyPress(keys: [.upArrow, .downArrow, .leftArrow, .rightArrow, .space, .return, .escape, .delete, .deleteForward]) {
      press in handle(press.key)
    }
    .onCopyCommand(perform: document.picked.isEmpty ? nil : { clipboardItems() })
    .onCutCommand(perform: document.picked.isEmpty ? nil : {
      let items = clipboardItems()
      document.remove(document.pickedInOrder)
      return items
    })
    .onPasteCommand(of: [.utf8PlainText]) { _ in CanvasClipboard.paste(into: document) }
    .focusedValue(\.canvasDocument, document)
    .onChange(of: document.linking) { old, new in announce(old: old, new: new) }
    .onChange(of: document.pickedLinks) { if !document.pickedLinks.isEmpty { canvasFocused = true } }
    .accessibilityElement(children: .contain)
    .accessibilityLabel(Text("Canvas"))
  }

  /// Tells VoiceOver which block a link being made from the keyboard leads to.
  private func announce(old: PendingLink?, new: PendingLink?) {
    guard let new, let target = new.target, let block = document.block(target) else { return }
    let step = document.step(of: target)
    let title = BlockInfo.title(block.kind.typeName)
    let text =
      old?.slot == new.slot
      ? String(localized: "Step \(step), \(title)")
      : String(localized: "Linking to step \(step), \(title). Choose another block with the arrow keys, then press Return to link or Escape to cancel.")
    AccessibilityNotification.Announcement(text).post()
  }

  private func describeTest() {
    if document.drafter == nil { document.drafter = TestDrafter() }
  }

  private func openPalette() {
    palette = PaletteTarget(slot: document.slotAfterSelection)
  }

  /// The block the last run of this test failed at.
  private var failedBlock: String? {
    runs.marks(for: document)?.marks.first { $0.value == .failed }?.key
  }

  /// Selects block `id`, scrolls it to the middle of the view and shows it in the inspector.
  private func showBlock(_ id: String) {
    document.pick([.block(id: id)], prefer: id)
    showsInspector = true
    canvasFocused = true
    guard let rect = document.layout.blocks.first(where: { $0.id == id })?.rect.cgRect else { return }
    scrollCanvas(
      to: CGPoint(
        x: max(0, rect.midX * zoom - viewport.width / 2), y: max(0, rect.midY * zoom - viewport.height / 2)),
      resized: false)
  }

  /// The palette next to where the canvas was right-clicked, inside the editor; otherwise at the top.
  private func paletteOffset(_ target: PaletteTarget, in box: CGSize) -> CGSize {
    let size = PaletteView.size, margin = 12.0
    guard let anchor = target.anchor else { return CGSize(width: (box.width - size.width) / 2, height: 24) }
    return CGSize(
      width: min(max(anchor.x, margin), max(margin, box.width - size.width - margin)),
      height: min(max(anchor.y, margin), max(margin, box.height - size.height - margin)))
  }

  /// A free place for a note or frame added without a point: right of the selected block, or near
  /// the top-left corner of what is in view.
  private var freeSpot: Point {
    if let id = document.selection, let rect = document.layout.blocks.first(where: { $0.id == id })?.rect {
      return Point(x: rect.x + rect.width + 2 * FlowCanvas.grid, y: rect.y)
    }
    return Point(x: scroll.x / zoom + 2 * FlowCanvas.grid, y: scroll.y / zoom + 2 * FlowCanvas.grid)
  }

  /// Adds a note signed with the git user's name.
  /// A Check at the end of the test, shown in the inspector, so the next run looks at the screen.
  private func addCheck() {
    document.insert("check", at: nil)
    showsInspector = true
    runs.dismissResult()
  }

  private var removalTitle: Text {
    Text("Delete the notes about this block too?")
  }

  private func removalMessage(_ removal: PendingRemoval) -> String {
    String(localized: "\(removal.notes.count) notes are about what you are deleting.")
  }

  private func addNote(at point: Point?) {
    let at = point ?? freeSpot
    Task { document.addNote(at: at, author: await project.git.authorName()) }
  }

  /// Adds a frame at `point`; without one, around the selection, or at a free place when nothing is selected.
  private func addFrame(at point: Point?) {
    let grid = FlowCanvas.grid
    let canvas = document.flow?.canvas
    let rects = document.pickedInOrder.compactMap { item -> CGRect? in
      switch item {
      case .block(let id): document.layout.blocks.first { $0.id == id }?.rect.cgRect
      case .section(let id): canvas?.sections.first { $0.id == id }?.rect.cgRect
      case .note(let id): canvas?.notes.first { $0.id == id }?.rect.cgRect
      }
    }
    if point == nil, let first = rects.first {
      let around = rects.dropFirst().reduce(first) { $0.union($1) }
      document.addFrame(Rect(
        x: around.minX - grid, y: around.minY - 3 * grid, width: around.width + 2 * grid, height: around.height + 4 * grid))
      return
    }
    let at = point ?? freeSpot
    document.addFrame(Rect(x: at.x, y: at.y, width: 26 * grid, height: 16 * grid))
  }

  private func clipboardItems() -> [NSItemProvider] {
    [NSItemProvider(object: document.copyPicked() as NSString)]
  }

  private func handle(_ key: KeyEquivalent) -> KeyPress.Result {
    guard palette == nil else { return .ignored }
    if document.linking != nil { return handleLinking(key) }
    switch key {
    case .space:
      openPalette()
    case .return:
      guard document.selection != nil || document.pickedNote != nil else { return .ignored }
      showsInspector = true
    case .escape:
      if inspectorShown.wrappedValue {
        showsInspector = false
      } else if !document.picked.isEmpty || !document.pickedLinks.isEmpty {
        document.pick([])
      } else {
        return .ignored
      }
    case .delete, .deleteForward:
      if !document.pickedLinks.isEmpty {
        document.unlink(document.pickedLinks.sorted { ($0.from, $0.edge) < ($1.from, $1.edge) })
      } else if !document.picked.isEmpty {
        document.remove(document.pickedInOrder)
      } else {
        return .ignored
      }
    default:
      guard let direction = Self.direction(of: key),
        let next = CanvasNavigation.next(
          from: document.selection, toward: direction, in: document.layout, order: document.order)
      else { return .ignored }
      document.selection = next
    }
    return .handled
  }

  /// Keys while a link is made from the keyboard: arrows choose the block, Return links, Escape cancels.
  private func handleLinking(_ key: KeyEquivalent) -> KeyPress.Result {
    switch key {
    case .return:
      if !document.finishLink() { NSSound.beep() }
    case .escape:
      document.cancelLink()
    default:
      guard let direction = Self.direction(of: key) else { return .ignored }
      document.moveLinkTarget(direction)
    }
    return .handled
  }

  private static func direction(of key: KeyEquivalent) -> CanvasDirection? {
    switch key {
    case .upArrow: .up
    case .downArrow: .down
    case .leftArrow: .left
    case .rightArrow: .right
    default: nil
    }
  }
}

private struct PaletteTarget: Equatable {
  let slot: Slot?
  /// Where on the canvas the block goes; `nil` to let the layout place it.
  var point: Point?
  /// Where in the editor the palette opens; `nil` for the top.
  var anchor: CGPoint?
}

enum Zoom {
  static let range = 0.3...2.0

  static func clamp(_ value: Double) -> Double {
    min(max(value, range.lowerBound), range.upperBound)
  }

  /// The next step up or down from `value`, within `range`.
  static func step(_ value: Double, up: Bool, in range: ClosedRange<Double> = range) -> Double {
    let steps = [0.05, 0.1, 0.2, 0.3, 0.5, 0.75, 1, 1.25, 1.5, 2].filter(range.contains)
    return up
      ? steps.first { $0 > value + 0.01 } ?? range.upperBound : steps.last { $0 < value - 0.01 } ?? range.lowerBound
  }

  /// The zoom that shows all of `content`, in canvas points, in `viewport` with `margin` around
  /// it; never above actual size, so a small test isn't blown up.
  static func fit(_ content: CGSize, in viewport: CGSize, margin: Double = 24) -> Double {
    guard content.width > 0, content.height > 0 else { return 1 }
    let width = (viewport.width - 2 * margin) / content.width
    let height = (viewport.height - 2 * margin) / content.height
    return clamp(min(1, width, height))
  }

  /// Where to scroll, when the zoom goes from `old` to `new`, so the canvas under `anchor`, a
  /// point of the visible part, stays under it; inside `content`, the zoomed canvas.
  static func offset(
    _ offset: CGPoint, keeping anchor: CGPoint, from old: Double, to new: Double, content: CGSize, viewport: CGSize
  ) -> CGPoint {
    let ratio = new / old
    func axis(_ offset: Double, _ anchor: Double, _ content: Double, _ visible: Double) -> Double {
      min(max((offset + anchor) * ratio - anchor, 0), max(content - visible, 0))
    }
    return CGPoint(
      x: axis(offset.x, anchor.x, content.width, viewport.width),
      y: axis(offset.y, anchor.y, content.height, viewport.height))
  }
}

/// The zoom of the canvas in view, for the View menu and the zoom bar.
/// Opens the block palette of the editor in front, for the Add Block command.
struct BlockPaletteOpener {
  let open: () -> Void
}

struct CanvasZoom {
  let value: Double
  let set: (Double) -> Void
  /// Zooms so the whole test is in view.
  let fit: () -> Void
  /// How far it zooms out and in.
  var range = Zoom.range
}

/// Tells a new test's author how to add the next block: a click, or Space on the canvas.
private struct AddBlockHint: View {
  let open: () -> Void

  var body: some View {
    Button(action: open) {
      HStack(spacing: 6) {
        Label("Add Block", systemImage: "plus")
        Text(verbatim: "·").foregroundStyle(.secondary).accessibilityHidden(true)
        Text("Space")
          .font(.caption.monospaced())
          .padding(.horizontal, 5)
          .background(.quaternary, in: .rect(cornerRadius: 4))
          .accessibilityHidden(true)
      }
    }
    .buttonStyle(.borderless)
    .controlSize(.small)
    .padding(.horizontal, 10)
    .padding(.vertical, 4)
    .adaptiveGlass(in: .capsule)
    .help(Text("Add a block after the selected one (Space or ⌘B)"))
  }
}

/// Zoom out, the zoom (a click goes to actual size), zoom in and fit, at the bottom of the canvas.
struct ZoomBar: View {
  let zoom: CanvasZoom
  var fitHelp = Text("Show the whole test (⌘0)")

  var body: some View {
    HStack(spacing: 2) {
      Button("Zoom Out", systemImage: "minus") { zoom.set(Zoom.step(zoom.value, up: false, in: zoom.range)) }
        .disabled(zoom.value <= zoom.range.lowerBound)
        .help(Text("Zoom out (⌘−)"))
      Button { zoom.set(1) } label: {
        Text(zoom.value, format: .percent.precision(.fractionLength(0)))
          .monospacedDigit()
          .frame(minWidth: 40)
      }
      .help(Text("Actual size"))
      .accessibilityLabel(Text("Zoom"))
      .accessibilityValue(Text(zoom.value, format: .percent.precision(.fractionLength(0))))
      .accessibilityHint(Text("Sets the zoom to actual size."))
      Button("Zoom In", systemImage: "plus") { zoom.set(Zoom.step(zoom.value, up: true, in: zoom.range)) }
        .disabled(zoom.value >= zoom.range.upperBound)
        .help(Text("Zoom in (⌘+)"))
      Button("Zoom to Fit", systemImage: "arrow.up.left.and.arrow.down.right") { zoom.fit() }
        .help(fitHelp)
    }
    .labelStyle(.iconOnly)
    .buttonStyle(.borderless)
    .controlSize(.small)
    .padding(.horizontal, 8)
    .padding(.vertical, 4)
    .adaptiveGlass(in: .capsule)
    .accessibilityElement(children: .contain)
    .accessibilityLabel(Text("Zoom"))
  }
}

/// How the test's newest run ended and how long ago; a click opens its report.
private struct LastRunChip: View {
  @Environment(ProjectStore.self) private var project
  let run: SavedRun?

  var body: some View {
    if let run {
      Button { project.showRun(run.url) } label: {
        TimelineView(.periodic(from: .now, by: 30)) { context in
          Label {
            Text(verbatim: "\(run.status.title) · \(Self.ago(run.startedAt, now: context.date))")
          } icon: {
            Image(systemName: run.status.symbol)
              .foregroundStyle(run.status.color)
          }
          .labelStyle(.titleAndIcon)
        }
      }
      .help(Text("Open the report of the last run"))
    } else {
      Label("Not run yet", systemImage: "circle.dashed")
        .labelStyle(.titleAndIcon)
        .foregroundStyle(.secondary)
        // Plain text has no button insets, so it would touch the edge of the glass group.
        .padding(.horizontal, 6)
        .help(Text("The test hasn’t been run yet"))
    }
  }

  /// "2 min ago", or "just now" within the first minute.
  static func ago(_ date: Date, now: Date) -> String {
    guard now.timeIntervalSince(date) >= 60 else { return String(localized: "just now") }
    return date.formatted(.relative(presentation: .numeric, unitsStyle: .abbreviated))
  }
}

extension FocusedValues {
  @Entry var canvasZoom: CanvasZoom?
  /// The test whose canvas has the keyboard focus.
  @Entry var canvasDocument: FlowDocument?
  @Entry var blockPalette: BlockPaletteOpener?
}
