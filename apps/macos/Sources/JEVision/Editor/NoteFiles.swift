import Foundation
import JEVisionCore
import UniformTypeIdentifiers

/// The files that come with notes. They are copied next to the test, so that they move, are
/// committed and go to the trash with it.
enum NoteFiles {
  /// Where Settings keeps the size, in megabytes, from which a file counts as large.
  static let largeKey = "noteLargeFileMegabytes"
  static let largeDefault = 10

  /// The size from which a file is large, in bytes; a megabyte is the million bytes Finder shows.
  static var largeSize: Int64 {
    let megabytes = UserDefaults.standard.object(forKey: largeKey) as? Int ?? largeDefault
    return Int64(max(megabytes, 1)) * 1_000_000
  }

  static func size(of url: URL) -> Int64 {
    Int64((try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0)
  }

  /// Where the file `name` of note `note` of the test at `test` is; `nil` for a name that can't be one.
  static func url(of name: String, note: String, test: URL) -> URL? {
    noteFilePath(test: path(test), note: note, name: name).map { URL(filePath: $0) }
  }

  /// Where a copy of `source` for note `note`, next to the test at `test`, goes: its own name,
  /// or another when a file in the note's folder has that name already.
  static func destination(for source: URL, note: String, test: URL) throws -> URL {
    guard let folder = noteFilesFolder(test: path(test), note: note) else {
      throw CocoaError(.fileWriteInvalidFileName)
    }
    let taken = (try? FileManager.default.contentsOfDirectory(atPath: folder)) ?? []
    let name = noteFileName(wanted: source.lastPathComponent, taken: taken)
    return URL(filePath: folder, directoryHint: .isDirectory).appending(path: name)
  }

  static func copy(_ source: URL, to destination: URL) throws {
    try FileManager.default.createDirectory(
      at: destination.deletingLastPathComponent(), withIntermediateDirectories: true)
    try FileManager.default.copyItem(at: source, to: destination)
  }

  /// What a file is, to pick its symbol and tell VoiceOver.
  enum Kind {
    case image, video, pdf, other

    init(_ url: URL) {
      let type = UTType(filenameExtension: url.pathExtension)
      if type?.conforms(to: .image) == true {
        self = .image
      } else if type?.conforms(to: .movie) == true {
        self = .video
      } else if type?.conforms(to: .pdf) == true {
        self = .pdf
      } else {
        self = .other
      }
    }

    var symbol: String {
      switch self {
      case .image: "photo"
      case .video: "film"
      case .pdf: "doc.richtext"
      case .other: "doc"
      }
    }
  }

  private static func path(_ url: URL) -> String {
    url.path(percentEncoded: false)
  }
}
