import AppKit
import ImageIO
import JEVisionCore
import JEVisionHands
import SwiftUI
import UniformTypeIdentifiers

extension AreaKind {
  var color: Color {
    switch self {
    case .compare: .accentColor
    case .ignore: .gray
    case .hide: .orange
    }
  }

  var title: String {
    switch self {
    case .compare: String(localized: "Compare")
    case .ignore: String(localized: "Ignore")
    case .hide: String(localized: "Hide")
    }
  }

  var explanation: String {
    switch self {
    case .compare: String(localized: "Only this part is compared. Without it, the whole image is.")
    case .ignore: String(localized: "Left out of the comparison: clocks, counters, avatars.")
    case .hide: String(localized: "Covered before a screenshot goes to an AI model or into a report: names, emails, card numbers.")
    }
  }

  /// Name of one area in the list and for VoiceOver, like “Ignored Area 2”.
  func name(_ index: Int) -> String {
    switch self {
    case .compare: String(localized: "Compare Area")
    case .ignore: String(localized: "Ignored Area \(index + 1)")
    case .hide: String(localized: "Hidden Area \(index + 1)")
    }
  }
}

/// A reference image read from disk.
struct ReferencePicture {
  let image: CGImage
  var pixels: CGSize { CGSize(width: image.width, height: image.height) }

  init?(url: URL) {
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
      let image = CGImageSourceCreateImageAtIndex(source, 0, nil)
    else { return nil }
    self.image = image
  }
}

/// The paths of reference pictures.
enum ReferenceFile {
  /// A free name for a new picture of block `block` in the `refs` folder next to the test.
  static func newPath(for block: String, in folder: URL) -> String {
    let base = block.replacing(/[^A-Za-z0-9_-]+/, with: "-")
    var name = base.isEmpty ? "reference" : base, number = 1
    while FileManager.default.fileExists(atPath: folder.appending(path: "refs/\(name).png").path) {
      number += 1
      name = "\(base)-\(number)"
    }
    return "refs/\(name).png"
  }
}

/// The reference picture with its areas drawn over it. Hidden areas are blurred, ignored ones
/// hatched, and everything outside the compare area dimmed.
struct ReferenceCanvas: View {
  let picture: ReferencePicture
  let reference: Reference
  var selection: AreaID?

  var body: some View {
    Canvas { context, size in
      let frame = ImageFrame(pixels: picture.pixels, bounds: size)
      let image = context.resolve(Image(decorative: picture.image, scale: 1))
      context.draw(image, in: frame.rect)
      if let area = reference.area {
        var outside = Path(frame.rect)
        outside.addRect(frame.view(area))
        context.fill(outside, with: .color(.black.opacity(0.35)), style: FillStyle(eoFill: true))
      }
      for id in reference.areaIDs {
        guard let r = reference[area: id] else { continue }
        Self.draw(id.kind, in: frame.view(r), image: image, frame: frame, selected: id == selection, context: context)
      }
    }
  }

  private static func draw(
    _ kind: AreaKind, in rect: CGRect, image: GraphicsContext.ResolvedImage, frame: ImageFrame, selected: Bool,
    context: GraphicsContext
  ) {
    let shape = Path(rect)
    switch kind {
    case .compare:
      context.fill(shape, with: .color(.accentColor.opacity(0.08)))
      context.stroke(shape, with: .color(.accentColor), lineWidth: selected ? 3 : 2)
    case .ignore:
      var hatch = context
      hatch.clip(to: shape)
      hatch.fill(shape, with: .color(.gray.opacity(0.2)))
      var lines = Path()
      var x = rect.minX - rect.height
      while x < rect.maxX {
        lines.move(to: CGPoint(x: x, y: rect.maxY))
        lines.addLine(to: CGPoint(x: x + rect.height, y: rect.minY))
        x += 8
      }
      hatch.stroke(lines, with: .color(.gray.opacity(0.6)), lineWidth: 3)
      context.stroke(shape, with: .color(.gray), style: StrokeStyle(lineWidth: selected ? 2.5 : 1, dash: [4, 3]))
    case .hide:
      var blurred = context
      blurred.clip(to: shape)
      blurred.addFilter(.blur(radius: 8))
      blurred.draw(image, in: frame.rect)
      context.fill(shape, with: .color(.orange.opacity(0.25)))
      context.stroke(shape, with: .color(.orange), lineWidth: selected ? 3 : 1.5)
    }
    if selected {
      for corner in AreaDrag.Corner.allCases {
        let p = corner.point(of: Rect(x: rect.minX, y: rect.minY, width: rect.width, height: rect.height))
        let handle = Path(roundedRect: CGRect(x: p.x - 4, y: p.y - 4, width: 8, height: 8), cornerRadius: 2)
        context.fill(handle, with: .color(.white))
        context.stroke(handle, with: .color(kind.color), lineWidth: 1.5)
      }
    }
  }
}

/// A small square of an area's color, for legends and lists.
struct AreaSwatch: View {
  let kind: AreaKind

  var body: some View {
    RoundedRectangle(cornerRadius: 3)
      .fill(kind.color.opacity(kind == .compare ? 0.15 : 0.45))
      .strokeBorder(kind.color, style: StrokeStyle(lineWidth: 1.5, dash: kind == .ignore ? [2, 2] : []))
      .frame(width: 12, height: 12)
      .accessibilityHidden(true)
  }
}

/// How much of the picture must match, with what that means in plain words.
struct SimilaritySlider: View {
  @Binding var threshold: Double

  var body: some View {
    LabeledContent {
      HStack {
        Slider(value: $threshold, in: 0.5...1) { Text("Similarity") }
          .labelsHidden()
        Text(BlockSummary.percent(threshold))
          .monospacedDigit()
          .frame(width: 44, alignment: .trailing)
      }
    } label: {
      Text("Similarity")
    }
    Text("The share of pixels that must look the same as in the reference. Around \(BlockSummary.percent(0.95)) lets small differences through, like smoothed text or a blinking cursor; \(BlockSummary.percent(1)) wants every pixel to match.")
      .font(.callout)
      .foregroundStyle(.secondary)
      .fixedSize(horizontal: false, vertical: true)
  }
}

/// The reference picture large, with tools to draw, move, resize and delete its areas, set the
/// similarity and replace the picture. Every change is saved to the test right away.
struct ReferenceAreaEditor: View {
  @Binding var reference: Reference
  /// Where the reference is in the block, like `reference` or `until.appears`.
  let field: String
  let context: FieldContext
  @Environment(ProjectStore.self) private var project
  @Environment(\.dismiss) private var dismiss

  private enum Appearance: Hashable { case light, dark }

  @State private var tool = AreaKind.ignore
  @State private var selection: AreaID?
  @State private var appearance = Appearance.light
  @State private var picture: ReferencePicture?
  @State private var isCapturing = false
  @State private var problem: Problem?
  /// The guide that replaces a picture other blocks use too.
  @State private var wizard: ReferenceWizard?
  /// Kind of the last change, so that changes of one kind in a row, like nudges, undo together.
  @State private var lastChange: String?
  @FocusState private var canvasFocused: Bool

  struct Problem: Identifiable {
    let id = UUID()
    let message: String
    var needsPermission = false
  }

  /// The picture's path, a labeled one included, for the appearance shown.
  private var imagePath: String {
    let image = RelativePath.path(of: reference.image, in: context.folder)
    return appearance == .dark && !image.isEmpty ? referenceDarkVariant(image: image) : image
  }

  private var imageURL: URL? {
    imagePath.isEmpty ? nil : RelativePath.resolve(imagePath, in: context.folder)
  }

  private var app: String { context.document.flow?.app ?? "" }

  init(reference: Binding<Reference>, field: String, context: FieldContext) {
    _reference = reference
    self.field = field
    self.context = context
    // Opens on the picture for the app's appearance, or on the one there is.
    let image = reference.wrappedValue.image
    let dark = NSApp.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
    let url = AppearanceImage.url(image, in: context.folder, dark: dark, variants: true)
    let darkImage = referenceDarkVariant(image: RelativePath.path(of: image, in: context.folder))
    let isDark = url != nil && url == RelativePath.resolve(darkImage, in: context.folder)
    _appearance = State(initialValue: isDark ? .dark : .light)
    _picture = State(initialValue: url.flatMap(ReferencePicture.init(url:)))
  }

  var body: some View {
    VStack(spacing: 0) {
      header
      Divider()
      HStack(spacing: 0) {
        canvas
          .frame(maxWidth: .infinity, maxHeight: .infinity)
          .background(.quaternary.opacity(0.5))
        Divider()
        sidebar
          .frame(width: 290)
      }
      Divider()
      footer
    }
    .frame(minWidth: 880, idealWidth: 1120, minHeight: 580, idealHeight: 760)
    .task(id: "\(imageURL?.path ?? "")#\(project.referenceChanges)") {
      picture = imageURL.flatMap(ReferencePicture.init(url:))
    }
    .onAppear { canvasFocused = true }
    .onExitCommand { dismiss() }
    .sheet(item: $wizard) { ReferenceWizardView(wizard: $0, undoManager: context.document.undoManager) }
    .alert(
      "The Picture Wasn’t Changed", isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } }),
      presenting: problem
    ) { problem in
      if problem.needsPermission {
        Button("Open System Settings") { Permission.screenRecording.openSettings() }
      }
      Button("OK", role: .cancel) {}
    } message: { problem in
      Text(problem.message)
    }
  }

  // MARK: Parts

  private var header: some View {
    HStack(spacing: 12) {
      VStack(alignment: .leading, spacing: 2) {
        Text("Reference Image")
          .font(.headline)
          .accessibilityAddTraits(.isHeader)
        Text(reference.image.isEmpty ? String(localized: "None") : imagePath)
          .font(.callout)
          .foregroundStyle(.secondary)
          .lineLimit(1)
          .truncationMode(.middle)
      }
      Spacer()
      if !reference.image.isEmpty {
        Picker(selection: $appearance) {
          Text("Light").tag(Appearance.light)
          Text("Dark").tag(Appearance.dark)
        } label: {
          Text("Appearance")
        }
        .pickerStyle(.segmented)
        .fixedSize()
        .help(Text("A run in dark appearance compares with the dark picture when there is one."))
      }
      if isCapturing {
        ProgressView()
          .controlSize(.small)
          .accessibilityLabel(Text("Taking a picture"))
      }
      Menu {
        Button("Choose a File…", action: chooseFile)
        Button(app.isEmpty ? String(localized: "Capture the App Window") : String(localized: "Capture the \(AppCatalog.shared.name(of: app)) Window"), action: capture)
          .disabled(app.isEmpty)
      } label: {
        Label(picture == nil ? "Add Image" : "Replace Image", systemImage: "photo.badge.arrow.down")
      }
      .fixedSize()
      .disabled(isCapturing)
    }
    .padding(.horizontal, 16)
    .padding(.vertical, 10)
  }

  @ViewBuilder
  private var canvas: some View {
    if let picture {
      AreaCanvas(
        picture: picture, reference: reference, tool: tool, selection: $selection, focus: $canvasFocused,
        label: Text("Reference image, \(picture.image.width) by \(picture.image.height) pixels"), change: change)
    } else {
      ContentUnavailableView {
        Label(appearance == .dark ? "No Dark Picture" : "No Reference Image", systemImage: appearance == .dark ? "moon" : "photo")
      } description: {
        if appearance == .dark {
          Text("In dark appearance the light picture is used. To add a dark one, switch the app to dark appearance and capture its window, or choose a file.")
        } else if reference.image.isEmpty {
          Text("Capture the app window as it should look, or choose a picture.")
        } else {
          Text("There’s no picture at \(imagePath). Capture the app window or choose a file to make one.")
        }
      } actions: {
        HStack {
          Button("Choose a File…", action: chooseFile)
          Button("Capture the App Window", action: capture)
            .disabled(app.isEmpty || isCapturing)
        }
      }
    }
  }

  private var sidebar: some View {
    Form {
      Section {
        Picker(selection: $tool) {
          ForEach(AreaKind.allCases, id: \.self) { kind in
            HStack(alignment: .firstTextBaseline, spacing: 8) {
              AreaSwatch(kind: kind)
              VStack(alignment: .leading, spacing: 2) {
                Text(kind.title)
                Text(kind.explanation)
                  .font(.callout)
                  .foregroundStyle(.secondary)
                  .fixedSize(horizontal: false, vertical: true)
              }
            }
            .tag(kind)
          }
        } label: {
          Text("Draw")
        }
        .pickerStyle(.radioGroup)
        .labelsHidden()
        Button("Add Area", action: addArea)
          .disabled(picture == nil)
      } header: {
        Text("Draw")
      } footer: {
        Text("Drag on the picture to draw an area of the chosen kind. Drag an area to move it, its corners to resize it.")
          .foregroundStyle(.secondary)
      }

      Section("Areas") {
        if reference.area == nil {
          HStack(spacing: 8) {
            AreaSwatch(kind: .compare)
            Text("The whole image is compared")
              .foregroundStyle(.secondary)
          }
        }
        ForEach(reference.areaIDs, id: \.self) { id in
          if let rect = reference[area: id] {
            AreaRow(
              id: id, rect: rect, picture: picture, isSelected: selection == id,
              select: {
                selection = id
                canvasFocused = true
              },
              delete: { remove(id) })
          }
        }
        if let selection, let rect = reference[area: selection], let picture {
          AreaPixelFields(rect: rect, frame: ImageFrame(pixels: picture.pixels, bounds: picture.pixels)) { new in
            change("field.\(selection)") { $0[area: selection] = new }
          }
        }
        IssueList(
          issues: context.issues(field + ".area") + context.issues(field + ".ignore") + context.issues(field + ".hide"))
      }

      Section("Match") {
        switch reference.compare {
        case .similar(let threshold):
          SimilaritySlider(
            threshold: Binding(
              get: { threshold },
              set: { new in change("threshold") { $0.compare = .similar(threshold: (new * 100).rounded() / 100) } }))
        case .exact:
          Text("Every pixel must match exactly. Choose Similar in the inspector to let small differences through.")
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        case .meaning:
          Text("An AI model decides whether the screen means the same. Hidden areas are covered before it sees the screen.")
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        }
      }
    }
    .formStyle(.grouped)
  }

  private var footer: some View {
    HStack {
      Text("Areas are the same for the light and dark pictures. Arrow keys move the selected area, Delete removes it.")
        .font(.callout)
        .foregroundStyle(.secondary)
      Spacer()
      let undo = context.document.undoManager
      Button("Undo") { undo?.undo() }
        .keyboardShortcut("z")
        .disabled(!(undo?.canUndo ?? false))
      Button("Redo") { undo?.redo() }
        .keyboardShortcut("z", modifiers: [.command, .shift])
        .disabled(!(undo?.canRedo ?? false))
      Button("Done") { dismiss() }
        .keyboardShortcut(.cancelAction)
        .buttonStyle(.borderedProminent)
    }
    .padding(.horizontal, 16)
    .padding(.vertical, 10)
  }

  // MARK: Changes

  /// Saves a change to the reference. Changes of the same `kind` in a row undo as one step;
  /// a `nil` kind is always a step of its own.
  private func change(_ kind: String?, _ edit: (inout Reference) -> Void) {
    if kind == nil || kind != lastChange { context.document.endUndoStep() }
    lastChange = kind
    var r = reference
    edit(&r)
    reference = r
  }

  private func remove(_ id: AreaID) {
    change(nil) { $0.remove(id) }
    selection = nil
  }

  private func addArea() {
    guard let picture else { return }
    let frame = ImageFrame(pixels: picture.pixels, bounds: picture.pixels)
    change(nil) { selection = $0.add(tool, frame.snapped(Rect(x: 0.35, y: 0.4, width: 0.3, height: 0.2))) }
    canvasFocused = true
  }

  // MARK: Replacing the picture

  private func chooseFile() {
    let panel = NSOpenPanel()
    panel.allowedContentTypes = [.png]
    panel.allowsMultipleSelection = false
    panel.directoryURL = context.folder
    guard panel.runModal() == .OK, let url = panel.url else { return }
    guard let png = try? Data(contentsOf: url), ReferencePicture(data: png) != nil else {
      problem = Problem(message: String(localized: "This file can’t be read as a PNG picture."))
      return
    }
    store(png, dark: appearance == .dark)
  }

  private func capture() {
    let app = app
    isCapturing = true
    Task {
      let result = await Task.detached(priority: .userInitiated) { () -> Result<Screenshot, HandsError> in
        Result { () throws(HandsError) in try MacHands(input: InputWatcher(), isCancelled: { false }).capture(app: app) }
      }.value
      isCapturing = false
      switch result {
      case .success(let shot):
        store(shot.png, dark: shot.dark)
        if !reference.image.isEmpty { appearance = shot.dark ? .dark : .light }
      case .failure(let error):
        problem = Self.problem(error, app: AppCatalog.shared.name(of: app))
      }
    }
  }

  static func problem(_ error: HandsError, app: String) -> Problem {
    switch error {
    case .PermissionDenied:
      Problem(
        message: String(localized: "JEVision needs the Screen Recording permission to take a picture of the app."),
        needsPermission: true)
    case .AppNotFound:
      Problem(message: String(localized: "\(app) isn’t installed on this Mac."))
    case .NoWindow:
      Problem(message: String(localized: "\(app) has no open window. Open it on the screen the test checks, then try again."))
    case .ScreenLocked:
      Problem(message: String(localized: "The screen is locked. Unlock it and try again."))
    case .Failed(let reason):
      Problem(message: String(localized: "The picture couldn’t be taken: \(reason)"))
    case .CannotOpen, .StillRunning, .WindowFixed, .WindowDiffers:
      Problem(message: String(localized: "The picture couldn’t be taken: \(error.localizedDescription)"))
    }
  }

  /// Makes `png` the picture: in place of the current one (light or dark), or as a new file in
  /// `refs` when the block has none yet. Undo brings the old picture back.
  private func store(_ png: Data, dark: Bool) {
    do {
      if reference.image.isEmpty {
        let path = ReferenceFile.newPath(for: context.block, in: context.folder)
        let file = RelativePath.resolve(path, in: context.folder)
        try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try png.write(to: file, options: .withoutOverwriting)
        let named = context.document.reference(to: file)
        let undo = context.document.undoManager
        undo?.beginUndoGrouping()
        change(nil) { $0.image = named.reference }
        context.document.trashOnUndo([file, URL(filePath: file.path(percentEncoded: false) + ".jevmeta")])
        undo?.endUndoGrouping()
        return
      }
      if case (_, .missing(let name))? = RelativePath.resolution(reference.image, in: context.folder) {
        problem = Problem(message: FileLabels.sentence("resource.missing", ["name": name]))
        return
      }
      let image = RelativePath.path(of: reference.image, in: context.folder)
      let file = RelativePath.resolve(dark ? referenceDarkVariant(image: image) : image, in: context.folder)
      guard let root = project.rootURL, RelativePath.isInside(file, folder: root) else {
        problem = Problem(
          message: String(localized: "The reference image is outside the project, so JEVision doesn’t change it. Choose a picture inside the project in the inspector first."))
        return
      }
      let reference = RelativePath.from(root, to: file)
      let picture = try TemporaryPicture(png)
      // A picture that other blocks use too is replaced through the guide that checks their settings.
      if let guide = project.referenceWizard(
        replacing: reference, with: picture, in: context.document.url, block: context.block)
      {
        wizard = guide
      } else {
        try project.acceptReference(picture, as: reference, undoManager: context.document.undoManager)
      }
      lastChange = nil
    } catch {
      problem = Problem(message: error.localizedDescription)
    }
  }
}

extension ReferencePicture {
  init?(data: Data) {
    guard let source = CGImageSourceCreateWithData(data as CFData, nil),
      let image = CGImageSourceCreateImageAtIndex(source, 0, nil)
    else { return nil }
    self.image = image
  }
}

/// The reference picture in the inspector, with its areas; a click opens the editor.
struct ReferenceThumbnail: View {
  let picture: ReferencePicture?
  let reference: Reference
  let open: () -> Void

  var body: some View {
    if let picture {
      Button(action: open) {
        ReferenceCanvas(picture: picture, reference: reference)
          .aspectRatio(picture.pixels, contentMode: .fit)
          .frame(maxWidth: .infinity, maxHeight: 160)
          .clipShape(.rect(cornerRadius: 6))
      }
      .buttonStyle(.plain)
      .accessibilityLabel(Text("Preview of \(BlockSummary.imageName(reference.image))"))
      .accessibilityHint(Text("Opens the reference editor"))
    }
  }
}

/// Which kinds of areas the reference has, in their colors.
struct AreaLegend: View {
  let reference: Reference

  var body: some View {
    HStack(spacing: 10) {
      item(.compare, count: reference.area == nil ? 0 : 1)
      item(.ignore, count: reference.ignore.count)
      item(.hide, count: reference.hide.count)
      if reference.area == nil, reference.ignore.isEmpty, reference.hide.isEmpty {
        Text("The whole image is compared")
          .foregroundStyle(.secondary)
      }
    }
    .font(.callout)
    .accessibilityElement(children: .combine)
  }

  @ViewBuilder
  private func item(_ kind: AreaKind, count: Int) -> some View {
    if count > 0 {
      HStack(spacing: 4) {
        AreaSwatch(kind: kind)
        Text(verbatim: count > 1 ? "\(kind.title) · \(count)" : kind.title)
      }
    }
  }
}
