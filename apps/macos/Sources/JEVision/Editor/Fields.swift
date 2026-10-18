import AppKit
import JEVisionCore
import SwiftUI
import UniformTypeIdentifiers

/// What a field needs to know about the block and test it edits.
struct FieldContext {
  let issues: [Issue]
  /// Folder of the test file: paths are relative to it.
  let folder: URL
  let projectRoot: URL?

  /// Problems with `field` or anything inside it, like `reference` for `reference.image`.
  func issues(_ field: String) -> [Issue] {
    issues.filter { issue in
      guard let f = issue.param("field") else { return false }
      return f == field || f.hasPrefix(field + ".")
    }
  }
}

/// Problems shown right under the field they are about.
struct IssueList: View {
  let issues: [Issue]

  var body: some View {
    ForEach(issues, id: \.self) { issue in
      let isError = issue.severity == .error
      Label {
        Text(IssueText.message(issue))
      } icon: {
        Image(systemName: isError ? "exclamationmark.octagon.fill" : "exclamationmark.triangle.fill")
      }
      .font(.callout)
      .foregroundStyle(isError ? .red : .orange)
      .fixedSize(horizontal: false, vertical: true)
    }
  }
}

/// A file chosen in Finder and stored relative to the test.
struct FilePathField: View {
  let label: Text
  @Binding var path: String
  let types: [UTType]
  let context: FieldContext

  private var url: URL { RelativePath.resolve(path, in: context.folder) }
  private var isWebAddress: Bool { path.hasPrefix("http://") || path.hasPrefix("https://") }

  var body: some View {
    LabeledContent {
      HStack {
        Text(path.isEmpty ? String(localized: "None") : path)
          .foregroundStyle(path.isEmpty ? .secondary : .primary)
          .truncationMode(.middle)
          .lineLimit(1)
          .textSelection(.enabled)
        Button("Choose…", action: choose)
      }
    } label: {
      label
    }
    if !path.isEmpty, !isWebAddress, path.first != "/" {
      if !FileManager.default.fileExists(atPath: url.path) {
        Warning(text: Text("There’s no such file next to the test."))
      } else if let root = context.projectRoot, !RelativePath.isInside(url, folder: root) {
        Warning(text: Text("This file is outside the project, so other people won’t get it with the tests."))
      }
    }
  }

  private func choose() {
    let panel = NSOpenPanel()
    panel.allowedContentTypes = types
    panel.allowsMultipleSelection = false
    panel.directoryURL = path.isEmpty || isWebAddress ? context.folder : url.deletingLastPathComponent()
    guard panel.runModal() == .OK, let chosen = panel.url else { return }
    path = RelativePath.from(context.folder, to: chosen)
  }
}

private struct Warning: View {
  let text: Text

  var body: some View {
    Label { text } icon: { Image(systemName: "exclamationmark.triangle.fill") }
      .font(.callout)
      .foregroundStyle(.orange)
      .fixedSize(horizontal: false, vertical: true)
  }
}

/// A picture of the chosen image, so it's clear what the block looks for.
struct ImagePreview: View {
  let path: String
  let context: FieldContext

  var body: some View {
    if !path.isEmpty, let image = NSImage(contentsOf: RelativePath.resolve(path, in: context.folder)) {
      Image(nsImage: image)
        .resizable()
        .scaledToFit()
        .frame(maxWidth: .infinity, maxHeight: 120)
        .clipShape(.rect(cornerRadius: 6))
        .accessibilityLabel(Text("Preview of \(BlockSummary.imageName(path))"))
    }
  }
}

/// Records one key combination: the next key pressed with its modifiers.
struct KeyRecorder: View {
  @Binding var keys: String
  @State private var monitor: Any?

  var body: some View {
    LabeledContent {
      Button(action: toggle) {
        if monitor != nil {
          Text("Press keys…")
        } else if keys.isEmpty {
          Text("Record Keys")
        } else {
          Text(verbatim: KeyCombo.display(keys)).monospaced()
        }
      }
      .accessibilityHint(Text("Then press the key combination. Escape cancels."))
    } label: {
      Text("Keys")
    }
    .onDisappear(perform: stop)
  }

  private func toggle() {
    if monitor != nil { return stop() }
    let keys = $keys
    monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
      let held = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
      MainActor.assumeIsolated {
        if event.keyCode == 53, held.intersection([.command, .shift, .option, .control]).isEmpty {
          stop()
        } else if let combo = KeyCombo.from(
          keyCode: event.keyCode, characters: event.characters(byApplyingModifiers: []), modifierFlags: held)
        {
          keys.wrappedValue = combo
          stop()
        }
      }
      return nil
    }
  }

  private func stop() {
    if let monitor { NSEvent.removeMonitor(monitor) }
    monitor = nil
  }
}

/// Text to type, with menus that insert secrets and data columns as placeholders.
struct PlaceholderTextField: View {
  @Binding var text: String
  let secrets: [String]
  let columns: [String]
  @State private var isNamingSecret = false
  @State private var secretName = ""

  var body: some View {
    TextField(text: $text, prompt: Text("Text to type"), axis: .vertical) { Text("Text") }
      .lineLimit(1...4)
    LabeledContent {
      Menu {
        Section("Secrets") {
          ForEach(secrets, id: \.self) { name in
            Button(name) { insert("secret", name) }
          }
          Button("New Secret…") {
            secretName = ""
            isNamingSecret = true
          }
        }
        if !columns.isEmpty {
          Section("Data Columns") {
            ForEach(columns, id: \.self) { name in
              Button(name) { insert("data", name) }
            }
          }
        }
      } label: {
        Text("Insert")
      }
      .fixedSize()
    } label: {
      Text("Insert a value that isn’t in the file")
        .foregroundStyle(.secondary)
    }
    .alert("New Secret", isPresented: $isNamingSecret) {
      TextField("Name", text: $secretName)
      Button("Insert") {
        let name = secretName.replacing(/[^A-Za-z0-9_]/, with: "_")
        if !name.isEmpty { insert("secret", name) }
      }
      Button("Cancel", role: .cancel) {}
    } message: {
      Text("Passwords and keys go here by name. Set the value in the test details: it stays in the Keychain and never gets into the file.")
    }
  }

  private func insert(_ scope: String, _ name: String) {
    text += "${\(scope).\(name)}"
  }
}

/// How long to wait before giving up, in seconds.
struct TimeoutField: View {
  @Binding var milliseconds: UInt64

  private var seconds: Binding<Double> {
    Binding(
      get: { Double(milliseconds) / 1000 },
      set: { milliseconds = UInt64((min(max($0, 0.1), 3600) * 1000).rounded()) })
  }

  var body: some View {
    LabeledContent {
      HStack(spacing: 6) {
        TextField(value: seconds, format: .number.precision(.fractionLength(0...1))) { Text("Time Limit") }
          .labelsHidden()
          .multilineTextAlignment(.trailing)
          .frame(width: 64)
        Stepper(value: seconds, in: 1...3600, step: 1) { Text("Time Limit") }
          .labelsHidden()
        Text("seconds")
      }
    } label: {
      Text("Time Limit")
    }
  }
}

/// A fraction of the window or image, entered as a percentage.
struct PercentField: View {
  let label: Text
  @Binding var value: Double

  var body: some View {
    TextField(value: $value, format: .percent.precision(.fractionLength(0...1))) { label }
  }
}

/// A reference image and how closely the screen must match it.
struct ReferenceEditor: View {
  @Binding var reference: Reference
  let field: String
  let context: FieldContext

  private enum Mode: Hashable { case exact, similar, meaning }

  private var mode: Binding<Mode> {
    Binding(
      get: {
        switch reference.compare {
        case .exact: .exact
        case .similar: .similar
        case .meaning: .meaning
        }
      },
      set: {
        switch $0 {
        case .exact: reference.compare = .exact
        case .similar: reference.compare = .similar(threshold: 0.95)
        case .meaning: reference.compare = .meaning
        }
      })
  }

  private var threshold: Binding<Double> {
    Binding(
      get: { if case .similar(let t) = reference.compare { t } else { 0.95 } },
      set: { reference.compare = .similar(threshold: ($0 * 100).rounded() / 100) })
  }

  private var hasArea: Binding<Bool> {
    Binding(
      get: { reference.area != nil },
      set: { reference.area = $0 ? Rect(x: 0, y: 0, width: 1, height: 1) : nil })
  }

  private func area(_ path: WritableKeyPath<Rect, Double>) -> Binding<Double> {
    Binding(
      get: { reference.area?[keyPath: path] ?? 0 },
      set: { reference.area?[keyPath: path] = $0 })
  }

  var body: some View {
    FilePathField(label: Text("Reference Image"), path: $reference.image, types: [.png], context: context)
    IssueList(issues: context.issues(field + ".image"))
    ImagePreview(path: reference.image, context: context)
    Picker(selection: mode) {
      Text("Exactly").tag(Mode.exact)
      Text("Similar").tag(Mode.similar)
      // Comparing by meaning needs a vision model, which the app can't set up yet: offered only
      // to show what a file already asks for.
      if case .meaning = reference.compare { Text("By Meaning").tag(Mode.meaning) }
    } label: {
      Text("Match")
    }
    if case .similar(let t) = reference.compare {
      LabeledContent {
        HStack {
          Slider(value: threshold, in: 0.5...1) { Text("Similarity") }
            .labelsHidden()
          Text(BlockSummary.percent(t))
            .monospacedDigit()
            .frame(width: 44, alignment: .trailing)
        }
      } label: {
        Text("Similarity")
      }
    }
    IssueList(issues: context.issues(field + ".threshold") + context.issues(field + ".compare"))
    DisclosureGroup {
      Toggle(isOn: hasArea) { Text("Compare only part of the image") }
      if reference.area != nil {
        PercentField(label: Text("From Left"), value: area(\.x))
        PercentField(label: Text("From Top"), value: area(\.y))
        PercentField(label: Text("Width"), value: area(\.width))
        PercentField(label: Text("Height"), value: area(\.height))
      }
      IssueList(issues: context.issues(field + ".area"))
    } label: {
      Text("More")
    }
  }
}

/// Where to click: an element found by its picture, or a fixed point in the window.
struct TargetEditor: View {
  @Binding var target: Target
  let field: String
  let context: FieldContext

  private var byLook: Binding<Bool> {
    Binding(
      get: { if case .image = target { true } else { false } },
      set: { target = $0 ? .image("") : .point(Point(x: 0.5, y: 0.5)) })
  }

  private var image: Binding<String> {
    Binding(
      get: { if case .image(let path) = target { path } else { "" } },
      set: { target = .image($0) })
  }

  private func coordinate(_ path: WritableKeyPath<Point, Double>) -> Binding<Double> {
    Binding(
      get: { if case .point(let p) = target { p[keyPath: path] } else { 0 } },
      set: { value in
        if case .point(var p) = target {
          p[keyPath: path] = value
          target = .point(p)
        }
      })
  }

  var body: some View {
    Picker(selection: byLook) {
      Text("By Look").tag(true)
      Text("By Position").tag(false)
    } label: {
      Text("Find")
    }
    .pickerStyle(.segmented)
    if byLook.wrappedValue {
      FilePathField(label: Text("Image"), path: image, types: [.png], context: context)
      ImagePreview(path: image.wrappedValue, context: context)
    } else {
      PercentField(label: Text("From Left"), value: coordinate(\.x))
      PercentField(label: Text("From Top"), value: coordinate(\.y))
    }
    IssueList(issues: context.issues(field))
  }
}
