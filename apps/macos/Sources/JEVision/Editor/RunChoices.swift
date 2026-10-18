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

  /// The data table the Data Rows menu offers rows of: the first one the test reads.
  static func dataTable(of flow: Flow, folder: URL) -> DataRows? {
    guard let file = DataColumns.files(in: flow).first else { return nil }
    return readDataRows(path: RelativePath.resolve(file, in: folder).path(percentEncoded: false))
  }

  /// One title per row, such as “Row 2: anna@example.com”, from the row's first filled value.
  static func rowTitles(of table: DataRows) -> [String] {
    table.rows.enumerated().map { index, row in
      guard let value = row.first(where: { !$0.trimmingCharacters(in: .whitespaces).isEmpty }) else {
        return String(localized: "Row \(index + 1)")
      }
      return String(localized: "Row \(index + 1): \(value)")
    }
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

  /// The row to run: a row the table no longer has means every row again.
  func row(in table: DataRows?) -> UInt32? {
    guard let dataRow, let table, dataRow < table.rows.count else { return nil }
    return UInt32(dataRow)
  }
}
