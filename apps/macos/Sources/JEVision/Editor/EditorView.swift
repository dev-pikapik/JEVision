import JEVisionCore
import SwiftUI
import UniformTypeIdentifiers

/// One test: the canvas in the middle, the selected block's inspector on the right.
struct EditorView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(RunCenter.self) private var runs
  @Environment(RecordCenter.self) private var records
  @Environment(\.undoManager) private var undoManager
  let document: FlowDocument
  @State private var zoom = 1.0
  @State private var pinchStart: Double?
  @State private var showsInspector = true
  @State private var showsDetails = false
  /// Where the palette adds a block; `nil` while it is closed.
  @State private var palette: PaletteTarget?
  /// How far the canvas is scrolled, in zoomed points.
  @State private var scroll = CGPoint.zero
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
    .onChange(of: undoManager) { document.undoManager = undoManager }
  }

  private func editor(_ flow: Flow) -> some View {
    canvas
      .overlay(alignment: .topLeading) {
        if let target = palette {
          GeometryReader { box in
            PaletteView { choice in
              palette = nil
              canvasFocused = true
              switch choice {
              case .block(let type):
                document.insert(type, at: target.slot, placedAt: target.point)
                showsInspector = true
              case .note: addNote(at: target.point)
              case .frame: addFrame(at: target.point)
              }
            } close: {
              palette = nil
              canvasFocused = true
            }
            .offset(paletteOffset(target, in: box.size))
          }
          .transition(.scale(scale: 0.96).combined(with: .opacity))
        }
      }
      .overlay(alignment: .bottom) {
        VStack(spacing: 0) {
          if let result = runs.result, result.test == document.url {
            RunResultBar(result: result)
              .transition(.move(edge: .bottom).combined(with: .opacity))
          }
          if let error = document.saveError {
            Label("Changes aren’t saved: \(error)", systemImage: "exclamationmark.triangle.fill")
              .padding(.horizontal, 14)
              .padding(.vertical, 8)
              .glassEffect(.regular.tint(.red.opacity(0.3)))
              .padding()
          }
        }
      }
      .animation(.snappy(duration: 0.2), value: palette)
      .animation(.snappy, value: runs.result)
      .onChange(of: document.flow) { runs.clearMarks(of: document) }
      .inspector(isPresented: inspectorShown) {
        Group {
          if let selection = document.selection {
            InspectorView(document: document, id: selection)
          }
        }
        .inspectorColumnWidth(min: 280, ideal: 320, max: 440)
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
          .accessibilityHint(Text("Shows the name, app, secrets and retries of the test."))
          .popover(isPresented: $showsDetails, arrowEdge: .bottom) {
            TestDetailsView(document: document)
          }
        }
        ToolbarItem(placement: .primaryAction) {
          Menu {
            Button("Add Note", systemImage: "note.text") { addNote(at: nil) }
            Button("Add Frame", systemImage: "rectangle.dashed") { addFrame(at: nil) }
            Divider()
            Button("Arrange Automatically", systemImage: "rectangle.3.group") { document.arrange() }
              .disabled(flow.canvas.positions.isEmpty)
          } label: {
            Label("Add Block", systemImage: "plus")
          } primaryAction: {
            openPalette()
          }
          .help(Text("Add a block after the selected one (Space)"))
        }
        ToolbarItem(placement: .primaryAction) {
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
          .disabled(runs.isRunning || records.isRecording || project.rootURL == nil)
          .help(Text("Run the test (⌘R)"))
        }
        ToolbarItem(placement: .primaryAction) {
          Button("Record", systemImage: "record.circle") { records.record(document) }
            .disabled(runs.isRunning || records.isRecording || project.rootURL == nil)
            .help(Text("Record steps by using the app; they go after the selected block (⇧⌘R)"))
        }
      }
      .focusedSceneValue(\.canvasZoom, $zoom)
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
      Text("As in Test (\(flow.retries))").tag(Int?.none)
      Divider()
      ForEach(RunChoices.retryChoices, id: \.self) { Text($0, format: .number).tag(Int?.some($0)) }
    } label: {
      Label(RunChoices.retriesTitle(document.runChoices.retries, test: flow.retries), systemImage: "arrow.clockwise")
    }
    .pickerStyle(.menu)
  }

  private func run(from block: String? = nil, stepByStep: Bool = false) {
    guard let root = project.rootURL else { return }
    runs.run(document, in: root, from: block, stepByStep: stepByStep)
  }

  private var inspectorShown: Binding<Bool> {
    Binding(get: { showsInspector && document.selection != nil }, set: { showsInspector = $0 })
  }

  private var canvas: some View {
    GeometryReader { viewport in
      ScrollView([.horizontal, .vertical]) {
        let visible = CGSize(width: viewport.size.width / zoom, height: viewport.size.height / zoom)
        FlowCanvas(document: document, appName: apps.name(of:), minSize: visible) { items, block in
          document.pick(items, prefer: block)
          showsInspector = true
          canvasFocused = true
        } onAdd: { slot in
          palette = PaletteTarget(slot: slot)
        } onPalette: { point, at in
          palette = PaletteTarget(
            slot: selectedSlot, point: point, anchor: CGPoint(x: at.x * zoom - scroll.x, y: at.y * zoom - scroll.y))
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
      .onScrollGeometryChange(for: CGPoint.self) { $0.contentOffset } action: { scroll = $1 }
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
    .gesture(
      MagnifyGesture()
        .onChanged { value in
          let start = pinchStart ?? zoom
          pinchStart = start
          zoom = Zoom.clamp(start * value.magnification)
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
    .accessibilityElement(children: .contain)
    .accessibilityLabel(Text("Canvas"))
  }

  private func openPalette() {
    palette = PaletteTarget(slot: selectedSlot)
  }

  /// Where a new block goes: after the selected block, or at the end without one.
  private var selectedSlot: Slot? {
    document.selection.flatMap { id in
      document.block(id).map { Slot(from: id, edge: $0.kind.typeName == "if" ? "then" : "next") }
    }
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
  private func addNote(at point: Point?) {
    let at = point ?? freeSpot
    Task { document.addNote(at: at, author: await project.git.identity().name) }
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
    let order = document.order
    switch key {
    case .space:
      openPalette()
    case .return:
      guard document.selection != nil else { return .ignored }
      showsInspector = true
    case .escape:
      if inspectorShown.wrappedValue { showsInspector = false } else if !document.picked.isEmpty { document.pick([]) } else { return .ignored }
    case .delete, .deleteForward:
      guard !document.picked.isEmpty else { return .ignored }
      document.remove(document.pickedInOrder)
    case .upArrow, .downArrow:
      guard !order.isEmpty else { return .ignored }
      let step = key == .downArrow ? 1 : -1
      let index = document.selection.flatMap(order.firstIndex) ?? (step > 0 ? -1 : order.count)
      document.selection = order[min(max(index + step, 0), order.count - 1)]
    default:
      guard let id = document.selection, let next = neighbour(of: id, right: key == .rightArrow) else { return .ignored }
      document.selection = next
    }
    return .handled
  }

  /// The closest block to the left or right, preferring ones at the same height.
  private func neighbour(of id: String, right: Bool) -> String? {
    let blocks = document.layout.blocks
    guard let from = blocks.first(where: { $0.id == id })?.rect else { return nil }
    return blocks
      .filter { right ? $0.rect.x > from.x : $0.rect.x < from.x }
      .min { a, b in
        func cost(_ r: Rect) -> Double { abs(r.y - from.y) * 2 + abs(r.x - from.x) }
        return cost(a.rect) < cost(b.rect)
      }?.id
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

  /// The next step up or down from `value`.
  static func step(_ value: Double, up: Bool) -> Double {
    let steps = [0.3, 0.5, 0.75, 1, 1.25, 1.5, 2]
    return up ? steps.first { $0 > value + 0.01 } ?? range.upperBound : steps.last { $0 < value - 0.01 } ?? range.lowerBound
  }
}

extension FocusedValues {
  @Entry var canvasZoom: Binding<Double>?
  /// The test whose canvas has the keyboard focus.
  @Entry var canvasDocument: FlowDocument?
}
