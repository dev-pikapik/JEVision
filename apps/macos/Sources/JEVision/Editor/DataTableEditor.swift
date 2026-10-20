import AppKit
import JEVisionCore
import SwiftUI

/// Opens the table of a Use Data block for viewing and editing.
struct DataTableButton: View {
  let file: String
  let context: FieldContext
  @State private var isEditing = false

  var body: some View {
    if !file.isEmpty, !file.contains("${") {
      let exists = FileManager.default.fileExists(atPath: RelativePath.resolve(file, in: context.folder).path)
      LabeledContent {
        Button(exists ? "Edit Data…" : "Create Table…") { isEditing = true }
          .sheet(isPresented: $isEditing) { DataTableEditor(file: file, context: context) }
      } label: {
        Text("Table")
      }
    }
  }
}

/// The CSV table of a Use Data block: its cells, rows and columns, saved back with the file's own
/// delimiter, line endings and byte order mark.
struct DataTableEditor: View {
  let file: String
  let context: FieldContext
  @Environment(\.dismiss) private var dismiss

  @State private var table: DataRows
  /// The table as the file has it; `nil` when there is no file yet.
  @State private var saved: DataRows?
  /// Why the file can't be shown, when it isn't a table.
  @State private var broken: String?
  @State private var saveError: String?
  @State private var confirmsDiscard = false

  private static let cellWidth: CGFloat = 170
  private static let numberWidth: CGFloat = 52

  private var url: URL { RelativePath.resolve(file, in: context.folder) }
  private var isChanged: Bool { table != saved }

  init(file: String, context: FieldContext) {
    self.file = file
    self.context = context
    let url = RelativePath.resolve(file, in: context.folder)
    if !FileManager.default.fileExists(atPath: url.path) {
      _table = State(initialValue: DataRows(columns: ["name"], rows: [[""]]))
      return
    }
    do {
      let table = try readDataRows(path: url.path(percentEncoded: false))
      _table = State(initialValue: table)
      _saved = State(initialValue: table)
    } catch {
      _table = State(initialValue: DataRows(columns: [], rows: []))
      _broken = State(initialValue: Self.message(error))
    }
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
  }

  // MARK: Parts

  private var header: some View {
    HStack(spacing: 12) {
      VStack(alignment: .leading, spacing: 2) {
        Text("Data Table")
          .font(.headline)
          .accessibilityAddTraits(.isHeader)
        Text(file)
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
        Button("Add Row", systemImage: "plus", action: addRow)
        Button("Add Column", systemImage: "plus.rectangle.portrait", action: addColumn)
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
            Button("Insert Column Before") { insertColumn(at: column) }
            Button("Insert Column After") { insertColumn(at: column + 1) }
            Divider()
            Button("Delete Column", role: .destructive) { deleteColumn(column) }
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
      Button("Delete Row \(row + 1)", systemImage: "minus.circle") { table.rows.remove(at: row) }
        .labelStyle(.iconOnly)
        .buttonStyle(.borderless)
        .foregroundStyle(.secondary)
        .help(Text("Delete this row"))
        .frame(width: 36)
    }
    .padding(.vertical, 5)
    .background(row.isMultiple(of: 2) ? Color.clear : Color.primary.opacity(0.03))
    .contextMenu {
      Button("Insert Row Above") { table.rows.insert(blankRow, at: row) }
      Button("Insert Row Below") { table.rows.insert(blankRow, at: row + 1) }
      Divider()
      Button("Delete Row", role: .destructive) { table.rows.remove(at: row) }
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
        if broken == nil, isChanged, saved != nil { confirmsDiscard = true } else { dismiss() }
      }
      .keyboardShortcut(.cancelAction)
      if broken == nil {
        Button("Save", action: save)
          .keyboardShortcut(.defaultAction)
          .buttonStyle(.borderedProminent)
          .disabled(!isChanged)
      }
    }
    .padding(.horizontal, 16)
    .padding(.vertical, 10)
  }

  // MARK: Editing

  private var blankRow: [String] { Array(repeating: "", count: table.columns.count) }

  private func cell(_ row: Int, _ column: Int) -> Binding<String> {
    Binding(
      get: { row < table.rows.count && column < table.rows[row].count ? table.rows[row][column] : "" },
      set: { value in
        guard row < table.rows.count, column < table.rows[row].count else { return }
        table.rows[row][column] = value
        saveError = nil
      })
  }

  private func columnName(_ column: Int) -> Binding<String> {
    Binding(
      get: { column < table.columns.count ? table.columns[column] : "" },
      set: { value in
        guard column < table.columns.count else { return }
        table.columns[column] = value
        saveError = nil
      })
  }

  private func addRow() {
    table.rows.append(blankRow)
  }

  private func addColumn() {
    insertColumn(at: table.columns.count)
  }

  private func insertColumn(at index: Int) {
    let taken = Set(table.columns.map { $0.trimmingCharacters(in: .whitespaces) })
    let name = ((table.columns.count + 1)...).lazy.map { "column\($0)" }.first { !taken.contains($0) } ?? "column"
    table.columns.insert(name, at: index)
    for row in table.rows.indices { table.rows[row].insert("", at: index) }
  }

  private func deleteColumn(_ index: Int) {
    guard table.columns.count > 1 else { return }
    table.columns.remove(at: index)
    for row in table.rows.indices { table.rows[row].remove(at: index) }
  }

  private func save() {
    do {
      try writeDataRows(path: url.path(percentEncoded: false), table: table)
      saved = table
      context.document.revalidate()
      dismiss()
    } catch {
      saveError = String(localized: "The table wasn’t saved: \(Self.message(error)).")
    }
  }

  // MARK: Words

  private static func message(_ error: any Error) -> String {
    guard case DataFileError.Invalid(_, _, let message) = error else { return error.localizedDescription }
    return message
  }

  static func size(rows: Int, columns: Int) -> String {
    [String(localized: "\(rows) rows"), String(localized: "\(columns) columns")].formatted(.list(type: .and))
  }
}
