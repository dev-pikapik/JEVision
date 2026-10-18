import Foundation
import JEVisionCore

/// How the next runs of a test go, picked in the Run menu. Kept while the test is open; the
/// environment is chosen for the whole project instead.
struct RunChoices: Equatable {
  /// Row of the data table to run, counted from 0; `nil` runs every row.
  var dataRow: Int?
  /// How many more times to try a failed run; `nil` keeps the number in the test.
  var retries: Int?

  /// Retries the Run menu offers.
  static let retryChoices = Array(0...3)

  /// The data table the Data Rows menu offers rows of: the one the whole test runs once per
  /// row of, read by the first Use Data block on the way from the start.
  static func dataTable(of flow: Flow, folder: URL) -> DataRows? {
    guard let file = testDataFile(flow: flow) else { return nil }
    return try? readDataRows(path: RelativePath.resolve(file, in: folder).path(percentEncoded: false))
  }

  /// One title per row, such as “Row 2: anna”, from its `name` column or else its first.
  static func rowTitles(of table: DataRows) -> [String] {
    dataRowLabels(table: table).enumerated().map { index, label in
      rowTitle(number: index + 1, label: label)
    }
  }

  static func rowTitle(number: Int, label: String) -> String {
    label.isEmpty ? String(localized: "Row \(number)") : String(localized: "Row \(number): \(label)")
  }

  /// Title of the Data Rows menu for the current choice.
  static func dataTitle(row: Int?, of table: DataRows) -> String {
    guard let row, row < table.rows.count else { return String(localized: "Data Rows: All \(table.rows.count)") }
    return String(localized: "Data Rows: Row \(row + 1)")
  }

  /// Title of the Retries menu: the chosen number, or the test's own.
  static func retriesTitle(_ retries: Int?, test: UInt32) -> String {
    guard let retries else { return String(localized: "Retries: As in Test (\(test))") }
    return String(localized: "Retries: \(retries)")
  }

  /// The rows to run: a row the table no longer has means every row again.
  func rows(in table: DataRows?) -> DataRowRange? {
    guard let dataRow, let table, dataRow < table.rows.count else { return nil }
    return DataRowRange(first: UInt32(dataRow), last: UInt32(dataRow))
  }
}
