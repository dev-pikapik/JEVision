import AppKit
import JEVisionCore
import SwiftUI

/// The test drawn as a flowchart, at the positions the core laid out, with its frames and notes.
/// Blocks, frames and notes move by dragging; a block links to another by dragging from the dot
/// under it, or from the keyboard. Unscaled: the editor zooms it.
struct FlowCanvas: View {
  static let space = "flowCanvas"
  static let grid = canvasGrid()

  @Environment(RunCenter.self) private var runs
  @Environment(ProjectStore.self) private var project
  let document: FlowDocument
  let appName: (String) -> String
  /// The smallest size to fill: the visible part of the editor at the current zoom.
  let minSize: CGSize
  /// How each block did in the last saved run; a run started since replaces them.
  let outcomes: [String: BlockOutcome]
  /// Selects these items; the inspector shows the block, when there is one.
  let onPick: (Set<CanvasItem>, String?) -> Void
  let onAdd: (Slot) -> Void
  /// Opens the block palette for a block at a point on the canvas: on a right click on the empty
  /// canvas, with no slot, to add after the selection; where a link is dropped on the empty
  /// canvas, with the link's slot. Also gives the same point in this view.
  let onPalette: (Slot?, Point, CGPoint) -> Void

  /// The test while something is dragged: drawn, but saved only when the drag ends.
  @State private var preview: (flow: Flow, layout: FlowLayout)?
  @State private var link: LinkDrag?
  @State private var marquee: CGRect?
  /// The block whose link dots show because the pointer is over it.
  @State private var hovered: String?
  @State private var pointer: CGPoint?
  @State private var editing: CanvasItem?

  var body: some View {
    let flow = preview?.flow ?? document.flow
    let layout = preview?.layout ?? document.layout
    let inset = inset
    ZStack(alignment: .topLeading) {
      backdrop(inset: inset)
      content(canvas: flow?.canvas, layout: layout)
        .offset(x: inset)
    }
    .frame(
      width: max(layout.width + inset, minSize.width), height: max(layout.height, minSize.height),
      alignment: .topLeading)
    .onContinuousHover(coordinateSpace: .named(Self.space)) { phase in
      if case .active(let at) = phase { pointer = at } else { pointer = nil }
    }
    .coordinateSpace(.named(Self.space))
    .dropDestination(for: BlockDrag.self) { drags, location in
      guard let type = drags.first?.type else { return false }
      drop(type, at: onCanvas(location))
      return true
    }
    .onChange(of: document.picked) {
      if let editing, !document.picked.contains(editing) { self.editing = nil }
    }
  }

  /// How far right the test is drawn to center a narrow one: whole grid steps, so the grid lines up.
  private var inset: Double {
    let spare = max(0, minSize.width - document.layout.width) / 2
    return (spare / Self.grid).rounded(.down) * Self.grid
  }

  /// Adds a block dragged from the palette: after the block it was dropped on, otherwise after the
  /// selection, centered where it was dropped.
  private func drop(_ type: String, at point: CGPoint) {
    guard let flow = document.flow else { return }
    if let hit = document.layout.blocks.first(where: { $0.rect.cgRect.contains(point) }) {
      document.insert(type, at: slotAfterBlock(flow: flow, id: hit.id))
    } else {
      let size = document.layout.blocks.first?.rect
      document.insert(
        type, at: document.slotAfterSelection,
        placedAt: Point(x: point.x - (size?.width ?? 0) / 2, y: point.y - (size?.height ?? 0) / 2))
    }
  }

  /// A point of this view on the canvas the core lays out.
  private func onCanvas(_ point: CGPoint) -> CGPoint {
    CGPoint(x: point.x - inset, y: point.y)
  }

  private func backdrop(inset: Double) -> some View {
    GridDots(inset: inset)
      .overlay {
        RightClickCatcher {
          guard let pointer else { return }
          let at = onCanvas(pointer)
          onPalette(nil, Point(x: at.x, y: at.y), pointer)
        }
      }
      .contentShape(.rect)
      .gesture(marqueeGesture)
  }

  private func content(canvas: CanvasContent?, layout: FlowLayout) -> some View {
    ZStack(alignment: .topLeading) {
      frames(canvas?.sections ?? [])
      links(layout)
      blocks(layout)
      notes(canvas?.notes ?? [])
      ports(layout)
      handles(canvas)
      dragFeedback
    }
    .frame(width: layout.width, height: layout.height, alignment: .topLeading)
  }

  // MARK: Layers

  private func frames(_ sections: [CanvasSection]) -> some View {
    ForEach(sections, id: \.id) { section in
      let item = CanvasItem.section(id: section.id)
      FrameView(
        section: section,
        isPicked: document.picked.contains(item),
        isEditing: editing == item,
        title: Binding(
          get: { document.flow?.canvas.sections.first { $0.id == section.id }?.title ?? "" },
          set: { document.editFrame(section.id, title: $0) }),
        endEditing: { endEditing(item) }
      )
      .modifier(CanvasItemGestures(
        item: item, isEditing: editing == item, move: moveGesture(item), pick: { pick(item) },
        edit: { editing = item }))
      .contextMenu {
        Button("Rename", systemImage: "pencil") { editing = item }
        Divider()
        ItemMenu(actions: actions(for: item))
      }
      .accessibilityElement(children: .ignore)
      .accessibilityLabel(Text("Frame"))
      .accessibilityValue(Text(verbatim: section.title))
      .accessibilityAddTraits(document.picked.contains(item) ? [.isButton, .isSelected] : .isButton)
      .accessibilityAction(.default) { pick(item) }
      .accessibilityAction(named: Text("Rename")) { editing = item }
      .accessibilityAction(named: Text("Delete")) { actions(for: item).delete() }
      .offset(x: section.rect.x, y: section.rect.y)
    }
  }

  private func links(_ layout: FlowLayout) -> some View {
    ZStack(alignment: .topLeading) {
      Edges(layout: layout)
      Edges(layout: layout, only: document.pickedLinks)
      ForEach(layout.edges.filter { $0.points.count > 1 }, id: \.self) { edge in
        let slot = Slot(from: edge.from, edge: edge.edge)
        EdgeHit(
          edge: edge, label: linkLabel(edge), isPicked: document.pickedLinks.contains(slot),
          pick: { pickLink(slot) },
          delete: { document.unlink([slot]) })
      }
      ForEach(layout.frames, id: \.block) { frame in
        RoundedRectangle(cornerRadius: 16)
          .fill(BlockInfo.color(ofType: document.block(frame.block)?.kind.typeName ?? "").opacity(0.05))
          .strokeBorder(.separator, style: StrokeStyle(lineWidth: 1, dash: [5, 4]))
          .frame(width: frame.rect.width, height: frame.rect.height)
          .offset(x: frame.rect.x, y: frame.rect.y)
          .allowsHitTesting(false)
          .accessibilityHidden(true)
      }
      ForEach(layout.edges, id: \.self) { edge in
        if let name = document.block(edge.from)?.kind.branchName(edge.edge) {
          let at = BranchLabel.position(of: edge)
          BranchLabel(name: name)
            .offset(x: at.x, y: at.y)
        }
      }
    }
  }

  private func blocks(_ layout: FlowLayout) -> some View {
    let order = document.order
    let run = runs.marks(for: document) ?? (outcomes, nil)
    return ForEach(layout.blocks, id: \.id) { laid in
      if let block = document.block(laid.id) {
        let item = CanvasItem.block(id: laid.id)
        BlockNode(
          block: block,
          summary: BlockSummary.text(block.kind, appName: appName),
          issues: document.issues(of: laid.id),
          isSelected: document.picked.contains(item),
          isLinkTarget: (link?.target ?? document.linking?.target) == laid.id,
          step: document.step(of: laid.id),
          steps: order.count,
          mark: run.marks[laid.id],
          isCurrent: run.current == laid.id,
          hasBreakpoint: runs.hasBreakpoint(laid.id, in: document),
          select: { pick(item) },
          startLink: {
            onPick([item], laid.id)
            if !document.startLink() { NSSound.beep() }
          },
          toggleBreakpoint: { runs.toggleBreakpoint(laid.id, in: document) },
          runFromHere: runs.isRunning || project.rootURL == nil
            ? nil : { project.rootURL.map { runs.run(document, in: $0, from: laid.id) } },
          addCondition: {
            onPick([item], laid.id)
            document.addCondition()
          },
          disconnect: document.hasLinks(laid.id) ? { document.disconnect(laid.id) } : nil,
          actions: actions(for: item))
        .frame(width: laid.rect.width, height: laid.rect.height)
        .onHover { hover(laid.id, $0) }
        .gesture(moveGesture(item))
        .offset(x: laid.rect.x, y: laid.rect.y)
      }
    }
  }

  private func notes(_ notes: [Note]) -> some View {
    ForEach(notes, id: \.id) { note in
      let item = CanvasItem.note(id: note.id)
      NoteView(
        note: note,
        isPicked: document.picked.contains(item),
        isEditing: editing == item,
        text: Binding(
          get: { document.flow?.canvas.notes.first { $0.id == note.id }?.text ?? "" },
          set: { document.editNote(note.id, text: $0) }),
        endEditing: { endEditing(item) }
      )
      .frame(width: note.rect.width, height: note.rect.height)
      .modifier(CanvasItemGestures(
        item: item, isEditing: editing == item, move: moveGesture(item), pick: { pick(item) },
        edit: { editing = item }))
      .contextMenu {
        Button("Edit Text", systemImage: "pencil") { editing = item }
        Divider()
        ItemMenu(actions: actions(for: item))
      }
      .accessibilityElement(children: .ignore)
      .accessibilityLabel(Text("Note"))
      .accessibilityValue(Text(verbatim: [note.text, note.author].filter { !$0.isEmpty }.joined(separator: ". ")))
      .accessibilityAddTraits(document.picked.contains(item) ? [.isButton, .isSelected] : .isButton)
      .accessibilityAction(.default) { pick(item) }
      .accessibilityAction(named: Text("Edit Text")) { editing = item }
      .accessibilityAction(named: Text("Delete")) { actions(for: item).delete() }
      .offset(x: note.rect.x, y: note.rect.y)
    }
  }

  private func ports(_ layout: FlowLayout) -> some View {
    ZStack(alignment: .topLeading) {
      ForEach(layout.slots, id: \.slot) { laid in
        SlotButton(
          isPinned: layout.blocks.isEmpty || (document.selection != nil && laid.slot.from == document.selection),
          label: slotLabel(laid.slot)
        ) { onAdd(laid.slot) }
        .offset(x: laid.at.x - SlotButton.size / 2, y: laid.at.y - SlotButton.size / 2)
      }
      ForEach(layout.ports.filter { showsPorts(of: $0.slot.from) }, id: \.slot) { port in
        PortHandle(
          label: portLabel(port.slot), accessibilityLabel: portAccessibilityLabel(port.slot),
          isLinking: document.linking?.slot == port.slot
        ) { if !document.startLink(from: port.slot) { NSSound.beep() } }
          .onHover { hover(port.slot.from, $0) }
          .gesture(linkGesture(port))
          .offset(x: port.at.x - PortHandle.size / 2, y: port.at.y - PortHandle.size / 2)
      }
    }
  }

  private func handles(_ canvas: CanvasContent?) -> some View {
    let placed = (canvas?.sections.map { Placed(item: .section(id: $0.id), rect: $0.rect) } ?? [])
      + (canvas?.notes.map { Placed(item: .note(id: $0.id), rect: $0.rect) } ?? [])
    return ForEach(placed.filter { document.picked.contains($0.item) && editing != $0.item }) { placed in
      ResizeHandle()
        .gesture(resizeGesture(placed.item))
        .offset(
          x: placed.rect.x + placed.rect.width - ResizeHandle.size,
          y: placed.rect.y + placed.rect.height - ResizeHandle.size)
    }
  }

  @ViewBuilder
  private var dragFeedback: some View {
    if let (from, to) = link.map({ ($0.from, $0.to) }) ?? keyboardLink {
      Path { path in
        path.move(to: from)
        path.addLine(to: to)
      }
      .stroke(Color.accentColor, style: StrokeStyle(lineWidth: 2, lineCap: .round, dash: [6, 4]))
      .allowsHitTesting(false)
    }
    if let marquee {
      Rectangle()
        .fill(Color.accentColor.opacity(0.08))
        .overlay { Rectangle().strokeBorder(Color.accentColor, lineWidth: 1) }
        .frame(width: marquee.width, height: marquee.height)
        .offset(x: marquee.minX, y: marquee.minY)
        .allowsHitTesting(false)
    }
  }

  /// Where the link being made from the keyboard is drawn: from its dot to the top of the block it
  /// leads to.
  private var keyboardLink: (CGPoint, CGPoint)? {
    guard let linking = document.linking,
      let port = document.layout.ports.first(where: { $0.slot == linking.slot }),
      let target = document.layout.blocks.first(where: { $0.id == linking.target })
    else { return nil }
    return (CGPoint(x: port.at.x, y: port.at.y), CGPoint(x: target.rect.cgRect.midX, y: target.rect.y))
  }

  // MARK: Gestures

  /// Clicking selects the item alone; with Shift or Command, adds it to the selection or takes it
  /// out. While a link is made from the keyboard, clicking a block it may lead to links to it.
  private func pick(_ item: CanvasItem) {
    if document.linking != nil, let id = item.blockID, document.finishLink(at: id) { return }
    let adding = !NSEvent.modifierFlags.intersection([.shift, .command]).isEmpty
    onPick(adding ? document.picked.symmetricDifference([item]) : [item], item.blockID)
  }

  /// Clicking a link selects it alone; with Shift or Command, adds it to the selected links or takes it out.
  private func pickLink(_ slot: Slot) {
    let adding = !NSEvent.modifierFlags.intersection([.shift, .command]).isEmpty
    document.pickLinks(adding ? document.pickedLinks.symmetricDifference([slot]) : [slot])
  }

  private func endEditing(_ item: CanvasItem) {
    guard editing == item else { return }
    editing = nil
    document.endUndoStep()
  }

  private func hover(_ id: String, _ inside: Bool) {
    if inside { hovered = id } else if hovered == id { hovered = nil }
  }

  private func showsPorts(of id: String) -> Bool {
    if let slot = link?.slot ?? document.linking?.slot { return slot.from == id }
    return preview == nil && (hovered == id || document.picked.contains(.block(id: id)))
  }

  /// What a drag starting on `item` moves: the item first, so that it snaps to the grid, then
  /// the rest of the selection.
  private func dragged(_ item: CanvasItem) -> [CanvasItem] {
    [item] + document.pickedInOrder.filter { $0 != item }
  }

  private func moveGesture(_ item: CanvasItem) -> some Gesture {
    DragGesture(minimumDistance: 3, coordinateSpace: .named(Self.space))
      .onChanged { value in
        if !document.picked.contains(item) { onPick([item], item.blockID) }
        guard let flow = document.flow,
          let moved = moveOnCanvas(
            flow: flow, items: dragged(item), dx: value.translation.width, dy: value.translation.height)
        else {
          preview = nil
          return
        }
        preview = (moved, layoutFlow(flow: moved))
      }
      .onEnded { value in
        preview = nil
        document.move(dragged(item), dx: value.translation.width, dy: value.translation.height)
      }
  }

  private func linkGesture(_ port: LaidSlot) -> some Gesture {
    DragGesture(minimumDistance: 2, coordinateSpace: .named(Self.space))
      .onChanged { value in
        let to = onCanvas(value.location)
        var drag = link ?? LinkDrag(
          slot: port.slot, from: CGPoint(x: port.at.x, y: port.at.y), to: to,
          targets: Set(document.layout.blocks.map(\.id).filter {
            document.canConnect(port.slot.from, edge: port.slot.edge, to: $0)
          }))
        drag.to = to
        drag.target = document.layout.blocks.first { drag.targets.contains($0.id) && $0.rect.cgRect.contains(to) }?.id
        link = drag
      }
      .onEnded { value in
        defer { link = nil }
        guard let link else { return }
        if let target = link.target {
          document.connect(link.slot.from, edge: link.slot.edge, to: target)
        } else if hypot(link.to.x - link.from.x, link.to.y - link.from.y) > 2 * Self.grid,
          !document.layout.blocks.contains(where: { $0.rect.cgRect.contains(link.to) })
        {
          // Dropped on the empty canvas: choose the block the link leads to, placed there.
          let width = document.layout.blocks.first { $0.id == link.slot.from }?.rect.width ?? 0
          onPalette(link.slot, Point(x: link.to.x - width / 2, y: link.to.y), value.location)
        }
      }
  }

  private func resizeGesture(_ item: CanvasItem) -> some Gesture {
    DragGesture(minimumDistance: 1, coordinateSpace: .named(Self.space))
      .onChanged { value in
        guard let flow = document.flow, let rect = grown(item, by: value.translation),
          let resized = resizeOnCanvas(flow: flow, item: item, rect: rect)
        else { return }
        preview = (resized, layoutFlow(flow: resized))
      }
      .onEnded { value in
        preview = nil
        if let rect = grown(item, by: value.translation) { document.resize(item, to: rect) }
      }
  }

  /// The saved bounds of frame or note `item`, with the bottom-right corner moved by `delta`.
  private func grown(_ item: CanvasItem, by delta: CGSize) -> Rect? {
    guard let canvas = document.flow?.canvas else { return nil }
    let rect: Rect? =
      switch item {
      case .section(let id): canvas.sections.first { $0.id == id }?.rect
      case .note(let id): canvas.notes.first { $0.id == id }?.rect
      case .block: nil
      }
    return rect.map { Rect(x: $0.x, y: $0.y, width: $0.width + delta.width, height: $0.height + delta.height) }
  }

  private var marqueeGesture: some Gesture {
    DragGesture(minimumDistance: 4, coordinateSpace: .named(Self.space))
      .onChanged { value in
        let a = onCanvas(value.startLocation), b = onCanvas(value.location)
        marquee = CGRect(x: min(a.x, b.x), y: min(a.y, b.y), width: abs(a.x - b.x), height: abs(a.y - b.y))
      }
      .onEnded { _ in
        guard let marquee else { return }
        self.marquee = nil
        let found = items(in: marquee)
        onPick(NSEvent.modifierFlags.contains(.shift) ? document.picked.union(found) : found, nil)
      }
  }

  /// Blocks and notes the rectangle touches, and frames wholly inside it.
  private func items(in rect: CGRect) -> Set<CanvasItem> {
    let canvas = document.flow?.canvas
    let blocks = document.layout.blocks.filter { rect.intersects($0.rect.cgRect) }.map { CanvasItem.block(id: $0.id) }
    let notes = (canvas?.notes ?? []).filter { rect.intersects($0.rect.cgRect) }.map { CanvasItem.note(id: $0.id) }
    let frames = (canvas?.sections ?? []).filter { rect.contains($0.rect.cgRect) }.map { CanvasItem.section(id: $0.id) }
    return Set(blocks + notes + frames)
  }

  /// Menu actions for `item`: on the whole selection when it is part of it, otherwise on it alone.
  private func actions(for item: CanvasItem) -> ItemActions {
    let only = {
      if !document.picked.contains(item) { onPick([item], item.blockID) }
    }
    return ItemActions(
      copy: {
        only()
        CanvasClipboard.copy(document)
      },
      duplicate: {
        only()
        document.duplicate()
      },
      delete: {
        only()
        document.remove(document.pickedInOrder)
      },
      paste: item.blockID.map { id in
        {
          onPick([item], id)
          CanvasClipboard.paste(into: document)
        }
      })
  }

  private func slotLabel(_ slot: Slot) -> Text {
    guard let block = document.block(slot.from) else { return Text("Add a block at the start") }
    let title = BlockInfo.title(block.kind.typeName)
    if let branch = block.kind.branchName(slot.edge) { return Text("Add a block to \(branch) after \(title)") }
    switch slot.edge {
    case "body": return Text("Add a block inside \(title)")
    default: return Text("Add a block after \(title)")
    }
  }

  private func portAccessibilityLabel(_ slot: Slot) -> Text {
    let kind = document.block(slot.from)?.kind
    let title = BlockInfo.title(kind?.typeName ?? "")
    if let branch = kind?.branchName(slot.edge) { return Text("Link \(branch) of \(title) to a block") }
    switch slot.edge {
    case "body": return Text("Choose the first step inside \(title)")
    default: return Text("Choose the step after \(title)")
    }
  }

  /// What VoiceOver calls the link `edge`: the blocks it joins, by their step in reading order.
  private func linkLabel(_ edge: LaidEdge) -> Text {
    func name(_ id: String) -> String {
      let step = document.step(of: id)
      return String(localized: "step \(step), \(BlockInfo.title(document.block(id)?.kind.typeName ?? ""))")
    }
    let from = name(edge.from), to = name(edge.to)
    if let branch = document.block(edge.from)?.kind.branchName(edge.edge) {
      return Text("\(branch) link from \(from) to \(to)")
    }
    switch edge.edge {
    case "body": return Text("Link from \(from) to the first step inside it, \(to)")
    default: return Text("Link from \(from) to \(to)")
    }
  }

  private func portLabel(_ slot: Slot) -> Text {
    if let branch = document.block(slot.from)?.kind.branchName(slot.edge) {
      return Text("Drag to link \(branch) to a block")
    }
    return slot.edge == "body"
      ? Text("Drag to choose the first step inside the loop") : Text("Drag to choose the next step")
  }
}

/// An edge being dragged from its dot to a block.
private struct LinkDrag {
  let slot: Slot
  let from: CGPoint
  var to: CGPoint
  /// Blocks the edge may lead to.
  let targets: Set<String>
  /// The block under the pointer, when the edge may lead to it.
  var target: String?
}

private struct Placed: Identifiable {
  let item: CanvasItem
  let rect: Rect
  var id: CanvasItem { item }
}

extension Rect {
  var cgRect: CGRect { CGRect(x: x, y: y, width: width, height: height) }
}

/// Copies and pastes canvas items as the text of a test file, so they also go into another test.
@MainActor
enum CanvasClipboard {
  static func copy(_ document: FlowDocument) {
    let text = document.copyPicked()
    guard !text.isEmpty else { return }
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString(text, forType: .string)
  }

  /// Pastes after the selected block; beeps when the clipboard holds no copied blocks.
  static func paste(into document: FlowDocument) {
    guard let text = NSPasteboard.general.string(forType: .string), document.paste(text) else {
      NSSound.beep()
      return
    }
  }
}

/// Dots on every grid step, bolder every sixth, lined up with the blocks.
struct GridDots: View {
  /// Where the canvas starts in this view.
  let inset: Double

  var body: some View {
    Canvas { context, size in
      let step = FlowCanvas.grid
      var minor = Path(), major = Path()
      for x in stride(from: 0, to: size.width, by: step) {
        let column = Int(((x - inset) / step).rounded())
        for y in stride(from: 0, to: size.height, by: step) {
          if column % 6 == 0 && Int((y / step).rounded()) % 6 == 0 {
            major.addEllipse(in: CGRect(x: x - 1.25, y: y - 1.25, width: 2.5, height: 2.5))
          } else {
            minor.addEllipse(in: CGRect(x: x - 0.75, y: y - 0.75, width: 1.5, height: 1.5))
          }
        }
      }
      context.fill(minor, with: .style(.quaternary))
      context.fill(major, with: .style(.tertiary))
    }
    .accessibilityHidden(true)
  }
}

/// Arrows between blocks: lines with rounded corners and a head at the end.
struct Edges: View {
  let layout: FlowLayout
  /// Draws only these links, highlighted as selected; `nil` draws every link plainly.
  var only: Set<Slot>?

  var body: some View {
    Canvas { context, _ in
      let shading: GraphicsContext.Shading = only == nil ? .style(.secondary) : .color(.accentColor)
      for edge in layout.edges where edge.points.count > 1 {
        if let only, !only.contains(Slot(from: edge.from, edge: edge.edge)) { continue }
        let points = edge.points.map { CGPoint(x: $0.x, y: $0.y) }
        context.stroke(Self.line(points), with: shading, style: StrokeStyle(lineWidth: only == nil ? 1.5 : 3, lineJoin: .round))
        context.fill(Self.head(from: points[points.count - 2], to: points[points.count - 1]), with: shading)
      }
    }
    .frame(width: layout.width, height: layout.height)
    .allowsHitTesting(false)
    .accessibilityHidden(true)
  }

  static func line(_ points: [CGPoint]) -> Path {
    var path = Path()
    path.move(to: points[0])
    for i in 1..<points.count - 1 {
      let a = points[i - 1], b = points[i], c = points[i + 1]
      let room = min(hypot(b.x - a.x, b.y - a.y), hypot(c.x - b.x, c.y - b.y)) / 2
      path.addArc(tangent1End: b, tangent2End: c, radius: min(8, room))
    }
    path.addLine(to: points[points.count - 1])
    return path
  }

  static func head(from a: CGPoint, to b: CGPoint) -> Path {
    let angle = atan2(b.y - a.y, b.x - a.x)
    let length = 8.0, spread = 0.45
    var path = Path()
    path.move(to: b)
    path.addLine(to: CGPoint(x: b.x - length * cos(angle - spread), y: b.y - length * sin(angle - spread)))
    path.addLine(to: CGPoint(x: b.x - length * cos(angle + spread), y: b.y - length * sin(angle + spread)))
    path.closeSubpath()
    return path
  }
}

/// Where an arrow is clicked to select it and right-clicked to delete it.
private struct EdgeHit: View {
  let edge: LaidEdge
  let label: Text
  let isPicked: Bool
  let pick: () -> Void
  let delete: () -> Void

  var body: some View {
    let reach = 6.0
    let points = edge.points.map { CGPoint(x: $0.x, y: $0.y) }
    let bounds = Edges.line(points).boundingRect.insetBy(dx: -reach, dy: -reach)
    let line = Edges.line(points.map { CGPoint(x: $0.x - bounds.minX, y: $0.y - bounds.minY) })
    Color.clear
      .frame(width: bounds.width, height: bounds.height)
      .contentShape(line.strokedPath(StrokeStyle(lineWidth: 2 * reach, lineCap: .round, lineJoin: .round)))
      .onTapGesture(perform: pick)
      .contextMenu {
        Button("Delete Link", systemImage: "trash", role: .destructive, action: delete)
      }
      .accessibilityElement()
      .accessibilityLabel(label)
      .accessibilityAddTraits(isPicked ? [.isButton, .isSelected] : .isButton)
      .accessibilityAction(.default, pick)
      .accessibilityAction(named: Text("Delete Link"), delete)
      .offset(x: bounds.minX, y: bounds.minY)
  }
}

struct BranchLabel: View {
  let name: String

  /// The top-left corner of the label: beside the arrow, just below where it leaves the block.
  static func position(of edge: LaidEdge) -> CGPoint {
    guard let start = edge.points.first else { return .zero }
    return CGPoint(x: start.x + 6, y: start.y + 4)
  }

  var body: some View {
    Text(verbatim: name)
      .font(.caption2.weight(.semibold))
      .foregroundStyle(.secondary)
      .allowsHitTesting(false)
      .accessibilityHidden(true)
  }
}

/// Moving by dragging, selecting by clicking and editing by double-clicking a frame or note;
/// off while its text is edited, so the text can be selected.
private struct CanvasItemGestures<Move: Gesture>: ViewModifier {
  let item: CanvasItem
  let isEditing: Bool
  let move: Move
  let pick: () -> Void
  let edit: () -> Void

  func body(content: Content) -> some View {
    content
      .gesture(move, including: isEditing ? .subviews : .all)
      .gesture(TapGesture(count: 2).onEnded(edit), including: isEditing ? .subviews : .all)
      .simultaneousGesture(TapGesture().onEnded(pick), including: isEditing ? .subviews : .all)
  }
}

private struct ItemActions {
  let copy: () -> Void
  let duplicate: () -> Void
  let delete: () -> Void
  /// Pastes after this block; `nil` for frames and notes.
  let paste: (() -> Void)?
}

private struct ItemMenu: View {
  let actions: ItemActions

  var body: some View {
    Button("Copy", systemImage: "doc.on.doc", action: actions.copy)
    if let paste = actions.paste {
      Button("Paste After", systemImage: "doc.on.clipboard", action: paste)
    }
    Button("Duplicate", systemImage: "plus.square.on.square", action: actions.duplicate)
    Divider()
    Button("Delete", systemImage: "trash", role: .destructive, action: actions.delete)
  }
}

private struct BlockNode: View {
  let block: Block
  let summary: String
  let issues: [Issue]
  let isSelected: Bool
  /// A link being dragged would lead here when dropped.
  let isLinkTarget: Bool
  let step: Int
  let steps: Int
  let mark: RunState.Mark?
  let isCurrent: Bool
  let hasBreakpoint: Bool
  let select: () -> Void
  /// Starts linking this block to another one from the keyboard.
  let startLink: () -> Void
  let toggleBreakpoint: () -> Void
  /// `nil` while the test can't be run from here.
  let runFromHere: (() -> Void)?
  /// One more condition on this If, or a new If after this block.
  let addCondition: () -> Void
  /// Removes the links into and out of the block; `nil` when it has none.
  let disconnect: (() -> Void)?
  let actions: ItemActions
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @Environment(\.colorSchemeContrast) private var contrast

  private var type: String { block.kind.typeName }
  private var hasError: Bool { issues.contains { $0.severity == .error } }

  private var border: (color: Color, width: Double) {
    if isLinkTarget { return (.accentColor, 3) }
    if isCurrent { return (.orange, 2.5) }
    if isSelected { return (.accentColor, 2.5) }
    if mark == .failed { return (.red, 2) }
    return (.primary.opacity(contrast == .increased ? 0.6 : 0.1), 1)
  }

  private var runValue: String? {
    if isCurrent { return String(localized: "Running now") }
    switch mark {
    case .passed: return String(localized: "Passed")
    case .failed: return String(localized: "Failed")
    case .skipped: return String(localized: "Skipped")
    case nil: return nil
    }
  }

  var body: some View {
    HStack(spacing: 10) {
      IconTile(symbol: BlockInfo.symbol(type), color: BlockInfo.color(ofType: type), size: 34)
      VStack(alignment: .leading, spacing: 1) {
        Text(BlockInfo.title(type))
          .font(.headline)
          .lineLimit(1)
        Text(summary)
          .font(.subheadline)
          .foregroundStyle(.secondary)
          .lineLimit(2)
          .truncationMode(.middle)
      }
      Spacer(minLength: 0)
      if !issues.isEmpty {
        Image(systemName: hasError ? "exclamationmark.octagon.fill" : "exclamationmark.triangle.fill")
          .foregroundStyle(hasError ? .red : .orange)
          .help(issues.map(IssueText.line).joined(separator: "\n"))
      }
    }
    .padding(.horizontal, 12)
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .background(.background, in: .rect(cornerRadius: 14))
    .overlay {
      RoundedRectangle(cornerRadius: 14)
        .strokeBorder(border.color, lineWidth: border.width)
    }
    .overlay(alignment: .topTrailing) { badge }
    .overlay(alignment: .leading) {
      if hasBreakpoint {
        Button(action: toggleBreakpoint) {
          Image(systemName: "pause.fill")
            .font(.system(size: 9, weight: .bold))
            .foregroundStyle(.white)
            .frame(width: 18, height: 26)
            .background(.blue, in: UnevenRoundedRectangle(topLeadingRadius: 5, bottomLeadingRadius: 5, bottomTrailingRadius: 9, topTrailingRadius: 9))
        }
        .buttonStyle(.plain)
        .offset(x: -10)
        .help(Text("The run pauses before this block. Click to remove."))
        .accessibilityHidden(true)
      }
    }
    .shadow(color: isCurrent ? .orange.opacity(0.45) : .black.opacity(0.08), radius: isCurrent ? 10 : 3, y: 1)
    .animation(reduceMotion ? nil : .snappy, value: isCurrent)
    .contentShape(.rect(cornerRadius: 14))
    .onTapGesture(perform: select)
    .contextMenu {
      Button("Run from Here", systemImage: "play", action: { runFromHere?() })
        .disabled(runFromHere == nil)
      Toggle("Pause Here", systemImage: "pause", isOn: Binding(get: { hasBreakpoint }, set: { _ in toggleBreakpoint() }))
      Divider()
      Button(action: addCondition) {
        Label { addConditionTitle } icon: { Image(systemName: "arrow.triangle.branch") }
      }
      Button("Disconnect", systemImage: "scissors", action: { disconnect?() })
        .disabled(disconnect == nil)
      Divider()
      ItemMenu(actions: actions)
    }
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(Text("Step \(step) of \(steps), \(BlockInfo.title(type))"))
    .accessibilityValue(Text(
      ([runValue, summary, hasBreakpoint ? String(localized: "The run pauses here") : nil].compactMap { $0 }
        + issues.map(IssueText.line)).joined(separator: ". ")))
    .accessibilityAddTraits(isSelected ? [.isButton, .isSelected] : .isButton)
    .accessibilityAction(.default, select)
    .accessibilityAction(named: Text("Link to Another Block"), startLink)
    .accessibilityAction(named: Text("Run from Here")) { runFromHere?() }
    .accessibilityAction(named: hasBreakpoint ? Text("Don’t Pause Here") : Text("Pause Here"), toggleBreakpoint)
    .accessibilityAction(named: addConditionTitle, addCondition)
    .accessibilityActions {
      if let disconnect { Button("Disconnect", action: disconnect) }
    }
    .accessibilityAction(named: Text("Duplicate"), actions.duplicate)
    .accessibilityAction(named: Text("Delete"), actions.delete)
  }

  private var addConditionTitle: Text { type == "if" ? Text("Add Condition") : Text("Add If After") }

  @ViewBuilder
  private var badge: some View {
    if !isCurrent, let mark {
      let look: (symbol: String, color: Color) =
        switch mark {
        case .passed: ("checkmark.circle.fill", .green)
        case .failed: ("xmark.square.fill", .red)
        case .skipped: ("minus.circle.fill", .gray)
        }
      Image(systemName: look.symbol)
        .font(.system(size: 16))
        .symbolRenderingMode(.palette)
        .foregroundStyle(.white, look.color)
        .offset(x: 6, y: -6)
        .accessibilityHidden(true)
    }
  }
}

/// A titled frame that groups blocks into a section; only its title bar takes clicks, so the
/// blocks and the canvas inside stay reachable.
struct FrameView: View {
  let section: CanvasSection
  let isPicked: Bool
  let isEditing: Bool
  let title: Binding<String>
  let endEditing: () -> Void
  @FocusState private var focused: Bool
  @Environment(\.colorSchemeContrast) private var contrast

  var body: some View {
    titleBar
      .frame(width: section.rect.width, height: section.rect.height, alignment: .topLeading)
      .background {
        RoundedRectangle(cornerRadius: 16)
          .fill(Color.primary.opacity(0.03))
          .strokeBorder(isPicked ? Color.accentColor : Color.primary.opacity(contrast == .increased ? 0.6 : 0.15), lineWidth: isPicked ? 2 : 1)
          .allowsHitTesting(false)
      }
  }

  private var titleBar: some View {
    Group {
      if isEditing {
        TextField(text: title, prompt: Text("Frame title")) { Text("Frame title") }
          .textFieldStyle(.plain)
          .focused($focused)
          .onAppear { focused = true }
          .onSubmit(endEditing)
          .onChange(of: focused) { if !focused { endEditing() } }
      } else if section.title.isEmpty {
        Text("Untitled Frame").foregroundStyle(.secondary)
      } else {
        Text(verbatim: section.title).lineLimit(1)
      }
    }
    .font(.headline)
    .padding(.horizontal, 14)
    .frame(maxWidth: .infinity, alignment: .leading)
    .frame(height: 36)
    .contentShape(.rect)
  }
}

/// A sticky note: its text, who wrote it and when.
struct NoteView: View {
  let note: Note
  let isPicked: Bool
  let isEditing: Bool
  let text: Binding<String>
  let endEditing: () -> Void
  @FocusState private var focused: Bool

  private static let paper = Color(nsColor: NSColor(name: nil) { appearance in
    appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
      ? NSColor(srgbRed: 0.29, green: 0.26, blue: 0.125, alpha: 1)
      : NSColor(srgbRed: 1, green: 0.965, blue: 0.78, alpha: 1)
  })
  private static let ink = Color(nsColor: NSColor(name: nil) { appearance in
    appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
      ? NSColor(srgbRed: 1, green: 0.953, blue: 0.753, alpha: 1)
      : NSColor(srgbRed: 0.24, green: 0.21, blue: 0, alpha: 1)
  })

  private var written: Date? { try? Date(note.time, strategy: .iso8601) }

  private var initials: String {
    note.author.split(separator: " ").prefix(2).compactMap(\.first).map(String.init).joined().uppercased()
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      Group {
        if isEditing {
          TextEditor(text: text)
            .scrollContentBackground(.hidden)
            .focused($focused)
            .onAppear { focused = true }
            .onChange(of: focused) { if !focused { endEditing() } }
        } else if note.text.isEmpty {
          Text("Double-click to write a note").opacity(0.6)
        } else {
          Text(verbatim: note.text)
        }
      }
      .font(.callout)
      .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
      if !note.author.isEmpty || written != nil {
        HStack(spacing: 6) {
          if !note.author.isEmpty {
            Text(verbatim: initials)
              .font(.system(size: 9, weight: .bold))
              .frame(width: 18, height: 18)
              .background(Self.ink.opacity(0.15), in: .circle)
            Text(verbatim: note.author)
          }
          if let written {
            Text(written, format: .relative(presentation: .named))
          }
        }
        .font(.caption)
        .lineLimit(1)
        .opacity(0.75)
      }
    }
    .padding(12)
    .foregroundStyle(Self.ink)
    .background(Self.paper, in: .rect(cornerRadius: 14))
    .overlay {
      RoundedRectangle(cornerRadius: 14)
        .strokeBorder(isPicked ? Color.accentColor : .clear, lineWidth: 2)
    }
    .shadow(color: .black.opacity(0.1), radius: 7, y: 4)
    .contentShape(.rect(cornerRadius: 14))
  }
}

/// The dot under a block an arrow is dragged from to link it to another block.
private struct PortHandle: View {
  static let size = 22.0
  let label: Text
  let accessibilityLabel: Text
  /// A link is being made from this dot from the keyboard.
  let isLinking: Bool
  /// Starts a link from this dot, to finish with the keyboard or a click on a block.
  let startLink: () -> Void

  var body: some View {
    Circle()
      .fill(isLinking ? AnyShapeStyle(Color.accentColor) : AnyShapeStyle(.background))
      .overlay { Circle().strokeBorder(Color.accentColor, lineWidth: 1.5) }
      .frame(width: 10, height: 10)
      .frame(width: Self.size, height: Self.size)
      .contentShape(.circle)
      .pointerStyle(.grabIdle)
      .onTapGesture(perform: startLink)
      .help(label)
      .accessibilityElement()
      .accessibilityLabel(accessibilityLabel)
      .accessibilityHint(Text("Then choose a block with the arrow keys and press Return."))
      .accessibilityAddTraits(isLinking ? [.isButton, .isSelected] : .isButton)
      .accessibilityAction(.default, startLink)
  }
}

/// The corner of a selected frame or note that resizes it.
private struct ResizeHandle: View {
  static let size = 16.0

  var body: some View {
    Image(systemName: "arrow.up.left.and.arrow.down.right")
      .font(.system(size: 8, weight: .bold))
      .foregroundStyle(.white)
      .frame(width: Self.size, height: Self.size)
      .background(Color.accentColor, in: .rect(cornerRadius: 4))
      .contentShape(.rect)
      .pointerStyle(.frameResize(position: .bottomTrailing))
      .help(Text("Drag to resize"))
      .accessibilityHidden(true)
  }
}

/// The “+” on an arrow that adds a block there. Shown on hover and next to the selected block.
private struct SlotButton: View {
  static let size = 22.0
  let isPinned: Bool
  let label: Text
  let add: () -> Void
  @State private var isHovered = false

  var body: some View {
    Button(action: add) {
      Image(systemName: "plus")
        .font(.system(size: 11, weight: .bold))
        .frame(width: Self.size, height: Self.size)
        .background(.background, in: .circle)
        .overlay { Circle().strokeBorder(Color.accentColor, lineWidth: 1.5) }
        .foregroundStyle(Color.accentColor)
    }
    .buttonStyle(.plain)
    .opacity(isPinned || isHovered ? 1 : 0)
    .padding(10)
    .contentShape(.rect)
    .onHover { isHovered = $0 }
    .padding(-10)
    .help(label)
    .accessibilityLabel(label)
  }
}

/// Takes right clicks and Control-clicks on the empty canvas; every other click goes through to
/// the views around it.
private struct RightClickCatcher: NSViewRepresentable {
  let action: () -> Void

  func makeNSView(context: Context) -> CatcherView { CatcherView() }

  func updateNSView(_ view: CatcherView, context: Context) {
    view.action = action
  }

  final class CatcherView: NSView {
    var action: () -> Void = {}

    override func hitTest(_ point: NSPoint) -> NSView? {
      guard let event = NSApp.currentEvent, Self.isSecondary(event) else { return nil }
      return super.hitTest(point)
    }

    override func rightMouseDown(with event: NSEvent) {
      action()
    }

    override func mouseDown(with event: NSEvent) {
      if Self.isSecondary(event) { action() }
    }

    private static func isSecondary(_ event: NSEvent) -> Bool {
      switch event.type {
      case .rightMouseDown, .rightMouseUp: true
      case .leftMouseDown, .leftMouseUp: event.modifierFlags.contains(.control)
      default: false
      }
    }
  }
}
