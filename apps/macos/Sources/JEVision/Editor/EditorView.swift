import JEVisionCore
import SwiftUI

/// One test: the canvas in the middle, the selected block's inspector on the right.
struct EditorView: View {
  @Environment(ProjectStore.self) private var project
  @Environment(RunCenter.self) private var runs
  @Environment(\.undoManager) private var undoManager
  let document: FlowDocument
  @State private var zoom = 1.0
  @State private var pinchStart: Double?
  @State private var showsInspector = true
  @State private var showsDetails = false
  /// Where the palette adds a block; `nil` while it is closed.
  @State private var palette: PaletteTarget?
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
      .overlay(alignment: .top) {
        if let target = palette {
          PaletteView { type in
            document.insert(type, at: target.slot)
            palette = nil
            showsInspector = true
            canvasFocused = true
          } close: {
            palette = nil
            canvasFocused = true
          }
          .padding(.top, 24)
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
          Button("Add Block", systemImage: "plus") { openPalette() }
            .help(Text("Add a block after the selected one (Space)"))
        }
        ToolbarItem(placement: .primaryAction) {
          Menu {
            Button("Run Step by Step", systemImage: "forward.frame") { run(stepByStep: true) }
            if let selection = document.selection {
              Button("Run from Selected Block", systemImage: "arrow.down.to.line") { run(from: selection) }
            }
          } label: {
            Label("Run", systemImage: "play.fill")
          } primaryAction: {
            run()
          }
          .disabled(runs.isRunning || project.rootURL == nil)
          .help(Text("Run the test (⌘R)"))
        }
      }
      .focusedSceneValue(\.canvasZoom, $zoom)
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
        FlowCanvas(document: document, appName: apps.name(of:)) { id in
          document.selection = id
          showsInspector = true
          canvasFocused = true
        } onAdd: { slot in
          palette = PaletteTarget(slot: slot)
        }
        .scaleEffect(zoom, anchor: .topLeading)
        .frame(width: document.layout.width * zoom, height: document.layout.height * zoom, alignment: .topLeading)
        .frame(
          minWidth: viewport.size.width, minHeight: viewport.size.height, alignment: .top)
        .contentShape(.rect)
        .onTapGesture {
          document.selection = nil
          canvasFocused = true
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
    .accessibilityElement(children: .contain)
    .accessibilityLabel(Text("Canvas"))
  }

  private func openPalette() {
    palette = PaletteTarget(slot: document.selection.flatMap { id in
      document.block(id).map { Slot(from: id, edge: $0.kind.typeName == "if" ? "then" : "next") }
    })
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
      if inspectorShown.wrappedValue { showsInspector = false } else if document.selection != nil { document.selection = nil } else { return .ignored }
    case .delete, .deleteForward:
      guard let id = document.selection else { return .ignored }
      document.delete(id)
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
}
