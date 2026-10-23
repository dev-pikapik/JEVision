import AppKit
import JEVisionCore
import SwiftUI

/// Opens the table of a Use Data block for viewing and editing.
struct DataTableButton: View {
  let file: String
  let context: FieldContext
  @Environment(\.undoManager) private var undoManager
  @State private var isEditing = false

  var body: some View {
    if !file.isEmpty, !file.contains("${") {
      let url = RelativePath.resolve(file, in: context.folder)
      let exists = FileManager.default.fileExists(atPath: url.path)
      LabeledContent {
        Button(exists ? "Edit Data…" : "Create Table…") { isEditing = true }
          .sheet(isPresented: $isEditing) {
            DataTableEditor(
              document: DataTableDocument(url: url), file: file, context: context, undoManager: undoManager)
          }
      } label: {
        Text("Table")
      }
    }
  }
}

/// The table of a Use Data block being edited. Every change is a step for Undo of the editor, and
/// saving tells when the file changed somewhere else since it was read.
@MainActor
@Observable
final class DataTableDocument {
  /// How the file changed somewhere else since the editor read it.
  enum OutsideChange { case changed, created, removed }

  /// What a new table starts with.
  static let blank = DataRows(columns: ["name"], rows: [[""]])

  let url: URL
  private(set) var table: DataRows
  /// The table as the file has it; `nil` when there is no file yet.
  private(set) var saved: DataRows? = nil
  /// Why the file can't be shown, when it isn't a table.
  let broken: String?
  /// What the file held when it was read; `nil` when there was none.
  @ObservationIgnored private var read: Data?
  @ObservationIgnored weak var undoManager: UndoManager?
  /// Changes with the same key in a row, like typing into one cell, undo as one step.
  @ObservationIgnored private var coalescing: String?

  init(url: URL) {
    self.url = url
    read = FileUndo.contents(of: url)
    guard FileManager.default.fileExists(atPath: url.path(percentEncoded: false)) else {
      table = Self.blank
      broken = nil
      return
    }
    do {
      let table = try readDataRows(path: url.path(percentEncoded: false))
      self.table = table
      saved = table
      broken = nil
    } catch {
      table = DataRows(columns: [], rows: [])
      broken = Self.message(error)
    }
  }

  /// The table differs from the file, or from a new table as it starts.
  var hasUnsavedChanges: Bool { broken == nil && table != (saved ?? Self.blank) }
  /// There is something to save: a change, or a table with no file yet.
  var canSave: Bool { broken == nil && (saved == nil || table != saved) }

  // MARK: Changes

  private var blankRow: [String] { Array(repeating: "", count: table.columns.count) }

  func addRow() {
    insertRow(at: table.rows.count, action: String(localized: "Add Row"))
  }

  func insertRow(at index: Int, action: String = String(localized: "Insert Row")) {
    guard index <= table.rows.count else { return }
    var new = table
    new.rows.insert(blankRow, at: index)
    apply(new, action: action)
  }

  func deleteRow(_ index: Int) {
    guard table.rows.indices.contains(index) else { return }
    var new = table
    new.rows.remove(at: index)
    apply(new, action: String(localized: "Delete Row"))
  }

  func addColumn() {
    insertColumn(at: table.columns.count, action: String(localized: "Add Column"))
  }

  func insertColumn(at index: Int, action: String = String(localized: "Insert Column")) {
    guard index <= table.columns.count else { return }
    var new = table
    let taken = Set(new.columns.map { $0.trimmingCharacters(in: .whitespaces) })
    let name = ((new.columns.count + 1)...).lazy.map { "column\($0)" }.first { !taken.contains($0) } ?? "column"
    new.columns.insert(name, at: index)
    for row in new.rows.indices { new.rows[row].insert("", at: index) }
    apply(new, action: action)
  }

  func deleteColumn(_ index: Int) {
    guard table.columns.count > 1, table.columns.indices.contains(index) else { return }
    var new = table
    new.columns.remove(at: index)
    for row in new.rows.indices { new.rows[row].remove(at: index) }
    apply(new, action: String(localized: "Delete Column"))
  }

  func rename(column: Int, to name: String) {
    guard table.columns.indices.contains(column) else { return }
    var new = table
    new.columns[column] = name
    apply(new, action: String(localized: "Rename Column"), coalesce: "column.\(column)")
  }

  func set(_ value: String, row: Int, column: Int) {
    guard table.rows.indices.contains(row), table.rows[row].indices.contains(column) else { return }
    var new = table
    new.rows[row][column] = value
    apply(new, action: String(localized: "Edit Cell"), coalesce: "cell.\(row).\(column)")
  }

  private func apply(_ new: DataRows, action: String, coalesce: String? = nil) {
    let old = table
    guard new != old else { return }
    if coalesce == nil { endTyping() }
    if coalesce == nil || coalesce != coalescing, let undoManager {
      undoManager.registerUndo(withTarget: self) { document in
        MainActor.assumeIsolated { document.apply(old, action: action) }
      }
      undoManager.setActionName(action)
    }
    table = new
    coalescing = coalesce
  }

  /// Makes what is typed next a new step, even into the same cell: typing in it ended.
  func breakCoalescing() { coalescing = nil }

  /// Ends typing in a cell or a column name of the editor's window, as Tab does, before the
  /// table changes around it. What was typed stays its own step, and the text field drops its
  /// own typing steps: kept while the table changes, they made ⌘Z retype the cell instead of
  /// undoing the change. Undo and Redo leave the typing alone.
  private func endTyping() {
    guard let undoManager, !undoManager.isUndoing, !undoManager.isRedoing else { return }
    for window in NSApplication.shared.windows {
      guard (window.firstResponder as? NSText)?.isFieldEditor == true, window.undoManager === undoManager
      else { continue }
      window.makeFirstResponder(nil)
    }
  }

  // MARK: Saving

  /// How the file changed somewhere else since it was read, if it did.
  func outsideChange() -> OutsideChange? {
    let now = FileUndo.contents(of: url)
    guard now != read else { return nil }
    if read == nil { return .created }
    return now == nil ? .removed : .changed
  }

  /// Writes the table to the file as one step of `undoManager`, the one of the window the editor
  /// was opened from: its Undo puts back what the file held, or takes away the file this created.
  /// `then` runs after that Undo or Redo.
  func save(undoManager: UndoManager?, ask: @escaping FileUndo.Ask = FileUndo.askToReplace, then: @escaping () -> Void)
    throws
  {
    endTyping()
    let action = saved == nil ? String(localized: "Create Data Table") : String(localized: "Edit Data Table")
    let file = url.resolvingSymlinksInPath()
    let path = file.path(percentEncoded: false)
    try checkResourceWrite(file: path)
    try FileUndo.record(
      [file, file.appendingPathExtension("jevmeta")], action: action, in: undoManager, ask: ask,
      beforeRestore: { try checkResourceWrite(file: path) }, then: then
    ) {
      try writeDataRows(path: url.path(percentEncoded: false), table: table)
      _ = try updateResourceLabel(file: path)
    }
    saved = table
    read = FileUndo.contents(of: url)
  }

  /// Shows the table the file holds now in place of the one being edited, as a step for Undo,
  /// which brings the edits back.
  func useFileVersion() throws {
    let now = FileUndo.contents(of: url)
    let version = try readDataRows(path: url.path(percentEncoded: false))
    read = now
    saved = version
    apply(version, action: String(localized: "Use the File’s Version"))
  }

  static func message(_ error: any Error) -> String {
    guard case DataFileError.Invalid(_, _, let message) = error else { return error.localizedDescription }
    return message
  }
}

/// The CSV table of a Use Data block: its cells, rows and columns, saved back with the file's own
/// delimiter, line endings and byte order mark.
struct DataTableEditor: View {
  let file: String
  let context: FieldContext
  /// Undo of the window the table was opened from, which gets the saved change.
  let undoManager: UndoManager?
  @Environment(\.dismiss) private var dismiss
  /// The sheet's own Undo, for the changes not saved yet.
  @Environment(\.undoManager) private var sheetUndoManager

  @State private var document: DataTableDocument
  @State private var saveError: String?
  @State private var confirmsDiscard = false
  /// How the file changed somewhere else, when that stopped saving.
  @State private var outside: DataTableDocument.OutsideChange?

  private static let cellWidth: CGFloat = 170
  private static let numberWidth: CGFloat = 52

  private var table: DataRows { document.table }
  private var broken: String? { document.broken }
  private var url: URL { document.url }

  /// The editor of `document`, the table of `file`.
  init(document: DataTableDocument, file: String, context: FieldContext, undoManager: UndoManager?) {
    self.file = file
    self.context = context
    self.undoManager = undoManager
    _document = State(initialValue: document)
  }

  var body: some View {
    VStack(spacing: 0) {
      header
      Divider()
      if let broken {
        ContentUnavailableView {
          Label("The Table Can’t Be Opened", systemImage: "tablecells.badge.ellipsis")
        } description: {
          Text("Fix the file, then open the table again: \(broken).")
        } actions: {
          Button("Open in Another App") { NSWorkspace.shared.open(url) }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
      } else {
        grid
      }
      Divider()
      footer
    }
    .frame(minWidth: 640, idealWidth: 860, minHeight: 420, idealHeight: 560)
    .confirmationDialog("Discard the changes to the table?", isPresented: $confirmsDiscard, titleVisibility: .visible) {
      Button("Discard Changes", role: .destructive) { dismiss() }
      Button("Keep Editing", role: .cancel) {}
    }
    .alert(
      outsideTitle, isPresented: Binding(get: { outside != nil }, set: { if !$0 { outside = nil } }),
      presenting: outside
    ) { change in
      Button(change == .removed ? "Create It Again" : "Replace with This Table", role: .destructive, action: write)
      if change != .removed { Button("Use the File’s Version", action: useFileVersion) }
      Button("Keep Editing", role: .cancel) {}
    } message: { change in
      switch change {
      case .changed:
        Text("It was changed after you opened this table. Saving would replace those changes with this table.")
      case .created:
        Text("It was created after you opened this table. Saving would replace what it holds with this table.")
      case .removed: Text("It was deleted after you opened this table. Saving creates it again with this table.")
      }
    }
    .onChange(of: sheetUndoManager, initial: true) { document.undoManager = sheetUndoManager }
    .onChange(of: document.table) { saveError = nil }
    .onReceive(NotificationCenter.default.publisher(for: NSControl.textDidEndEditingNotification)) { note in
      // The text field dropped its own typing steps, so the table's step for that typing ends too.
      if (note.object as? NSView)?.window?.undoManager === sheetUndoManager { document.breakCoalescing() }
    }
    .onDisappear { sheetUndoManager?.removeAllActions(withTarget: document) }
  }

  private var outsideTitle: Text {
    let name = url.lastPathComponent
    return switch outside {
    case .created: Text("“\(name)” was created somewhere else")
    case .removed: Text("“\(name)” was deleted somewhere else")
    case .changed, nil: Text("“\(name)” was changed somewhere else")
    }
  }

  // MARK: Parts

  private var header: some View {
    HStack(spacing: 12) {
      VStack(alignment: .leading, spacing: 2) {
        Text("Data Table")
          .font(.headline)
          .accessibilityAddTraits(.isHeader)
        Text(RelativePath.path(of: file, in: context.folder))
          .font(.callout)
          .foregroundStyle(.secondary)
          .lineLimit(1)
          .truncationMode(.middle)
      }
      Spacer()
      if broken == nil {
        Text(Self.size(rows: table.rows.count, columns: table.columns.count))
          .font(.callout)
          .foregroundStyle(.secondary)
        Button("Add Row", systemImage: "plus", action: document.addRow)
        Button("Add Column", systemImage: "plus.rectangle.portrait", action: document.addColumn)
      }
    }
    .padding(.horizontal, 16)
    .padding(.vertical, 10)
  }

  private var grid: some View {
    ScrollView([.horizontal, .vertical]) {
      LazyVStack(alignment: .leading, spacing: 0, pinnedViews: [.sectionHeaders]) {
        Section {
          ForEach(table.rows.indices, id: \.self) { row in
            rowView(row)
          }
          if table.rows.isEmpty {
            Text("No rows yet. Each row runs the whole test once; add one with Add Row.")
              .foregroundStyle(.secondary)
              .padding(16)
          }
        } header: {
          columnHeaders
        }
      }
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
  }

  private var columnHeaders: some View {
    HStack(spacing: 0) {
      Text("Row")
        .frame(width: Self.numberWidth)
        .foregroundStyle(.secondary)
      ForEach(table.columns.indices, id: \.self) { column in
        HStack(spacing: 4) {
          TextField("Column Name", text: columnName(column))
            .textFieldStyle(.plain)
            .fontWeight(.semibold)
            .accessibilityLabel(Text("Name of column \(column + 1)"))
          Menu {
            Button("Insert Column Before") { document.insertColumn(at: column) }
            Button("Insert Column After") { document.insertColumn(at: column + 1) }
            Divider()
            Button("Delete Column", role: .destructive) { document.deleteColumn(column) }
              .disabled(table.columns.count == 1)
          } label: {
            Image(systemName: "ellipsis.circle")
          }
          .menuStyle(.borderlessButton)
          .menuIndicator(.hidden)
          .fixedSize()
          .accessibilityLabel(Text("Column \(table.columns[column]) actions"))
        }
        .padding(.horizontal, 8)
        .frame(width: Self.cellWidth, alignment: .leading)
        .overlay(alignment: .leading) { Divider() }
      }
      Spacer().frame(width: 36)
    }
    .padding(.vertical, 6)
    .background(.bar)
    .overlay(alignment: .bottom) { Divider() }
  }

  private func rowView(_ row: Int) -> some View {
    HStack(spacing: 0) {
      Text(row + 1, format: .number)
        .monospacedDigit()
        .foregroundStyle(.secondary)
        .frame(width: Self.numberWidth)
        .accessibilityLabel(Text("Row \(row + 1)"))
      ForEach(table.columns.indices, id: \.self) { column in
        TextField(text: cell(row, column), prompt: nil) { Text(verbatim: table.columns[column]) }
          .textFieldStyle(.plain)
          .padding(.horizontal, 8)
          .frame(width: Self.cellWidth, alignment: .leading)
          .overlay(alignment: .leading) { Divider() }
          .accessibilityLabel(Text("\(table.columns[column]), row \(row + 1)"))
      }
      Button("Delete Row \(row + 1)", systemImage: "minus.circle") { document.deleteRow(row) }
        .labelStyle(.iconOnly)
        .buttonStyle(.borderless)
        .foregroundStyle(.secondary)
        .help(Text("Delete this row"))
        .frame(width: 36)
    }
    .padding(.vertical, 5)
    .background(row.isMultiple(of: 2) ? Color.clear : Color.primary.opacity(0.03))
    .contextMenu {
      Button("Insert Row Above") { document.insertRow(at: row) }
      Button("Insert Row Below") { document.insertRow(at: row + 1) }
      Divider()
      Button("Delete Row", role: .destructive) { document.deleteRow(row) }
    }
  }

  private var footer: some View {
    HStack(alignment: .firstTextBaseline) {
      if let saveError {
        Label(saveError, systemImage: "exclamationmark.triangle.fill")
          .foregroundStyle(.red)
          .fixedSize(horizontal: false, vertical: true)
      } else if broken == nil {
        Text("Each row runs the whole test once. Reports name a row by its “name” column, or else by its first column.")
          .font(.callout)
          .foregroundStyle(.secondary)
      }
      Spacer()
      Button(broken == nil ? "Cancel" : "Close") {
        if document.hasUnsavedChanges { confirmsDiscard = true } else { dismiss() }
      }
      .keyboardShortcut(.cancelAction)
      if broken == nil {
        Button("Save", action: save)
          .keyboardShortcut(.defaultAction)
          .buttonStyle(.borderedProminent)
          .disabled(!document.canSave)
      }
    }
    .padding(.horizontal, 16)
    .padding(.vertical, 10)
  }

  // MARK: Editing

  private func cell(_ row: Int, _ column: Int) -> Binding<String> {
    Binding(
      get: { row < table.rows.count && column < table.rows[row].count ? table.rows[row][column] : "" },
      set: { document.set($0, row: row, column: column) })
  }

  private func columnName(_ column: Int) -> Binding<String> {
    Binding(
      get: { column < table.columns.count ? table.columns[column] : "" },
      set: { document.rename(column: column, to: $0) })
  }

  /// Saves, unless the file changed somewhere else since the table was read: then the person
  /// chooses first.
  private func save() {
    if let change = document.outsideChange() { outside = change } else { write() }
  }

  private func write() {
    do {
      try document.save(undoManager: undoManager) { [weak flow = context.document] in flow?.revalidate() }
      context.document.revalidate()
      dismiss()
    } catch {
      saveError = String(localized: "The table wasn’t saved: \(DataTableDocument.message(error)).")
    }
  }

  private func useFileVersion() {
    do {
      try document.useFileVersion()
    } catch {
      saveError = String(localized: "The file’s version can’t be shown: \(DataTableDocument.message(error)).")
    }
  }

  // MARK: Words

  static func size(rows: Int, columns: Int) -> String {
    [String(localized: "\(rows) rows"), String(localized: "\(columns) columns")].formatted(.list(type: .and))
  }
}
