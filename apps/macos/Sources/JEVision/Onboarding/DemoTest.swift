import Foundation

/// The ready test the first launch offers: it opens a copy of a short note in TextEdit, adds a
/// line, checks how the window looks and quits TextEdit. The same steps as examples/textedit.
enum DemoTest {
  static let folder = "Demo"
  static let file = "write-a-note.flow.json"
  static let note = "note.txt"

  /// Writes the demo into `project` unless it is already there, and returns its path relative to `project`.
  static func write(into project: URL) throws -> String {
    let dir = project.appending(path: folder, directoryHint: .isDirectory)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let test = dir.appending(path: file)
    if !FileManager.default.fileExists(atPath: test.path) {
      try Data(noteText.utf8).write(to: dir.appending(path: note))
      try flowData().write(to: test, options: .withoutOverwriting)
    }
    return folder + "/" + file
  }

  private static var noteText: String {
    String(localized: "Shopping list\n\n- apples\n- bread\n", comment: "The note the demo test opens in TextEdit")
  }

  static func flowData() throws -> Data {
    let flow: [String: Any] = [
      "format": 1,
      "name": String(localized: "Add a line to a note", comment: "Name of the demo test"),
      "description": String(
        localized: "Opens a copy of note.txt in TextEdit, adds a line at the end, checks how the window looks and quits TextEdit.",
        comment: "Description of the demo test"),
      "app": "com.apple.TextEdit",
      "tags": ["demo"],
      "start": "open",
      "blocks": [
        "open": ["type": "openFile", "files": [note], "app": "com.apple.TextEdit", "next": "end"],
        "end": ["type": "pressKeys", "keys": "cmd+down", "next": "type"],
        "type": ["type": "type", "text": String(localized: "- milk\n", comment: "The line the demo test types into the note"), "next": "check"],
        "check": [
          "type": "check", "reference": ["image": "refs/note.png", "compare": "similar", "threshold": 0.95], "next": "quit",
        ],
        "quit": ["type": "pressKeys", "keys": "cmd+q"],
      ],
    ]
    return try JSONSerialization.data(withJSONObject: flow, options: [.prettyPrinted, .sortedKeys])
  }
}
