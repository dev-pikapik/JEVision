import Foundation
import Testing

@testable import JEVisionCLI
@testable import JEVisionHands

struct AppCopiesTests {
  private func scratch() throws -> URL {
    let made = FileManager.default.temporaryDirectory.appending(path: "jevision-copies-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: made.appending(path: "Copy.app"), withIntermediateDirectories: true)
    try FileManager.default.createDirectory(at: made.appending(path: "Other.app"), withIntermediateDirectories: true)
    return made
  }

  @Test func theFlagNamesCopiesByAppWithAbsolutePaths() throws {
    let folder = try scratch()
    defer { try? FileManager.default.removeItem(at: folder) }
    let current = folder.path(percentEncoded: false)
    let copies = try Command.appCopies(
      ["com.example.App=Copy.app/", "io.github.other=\(current)/Other.app"], current: current)
    #expect(copies == ["com.example.App": "\(current)/Copy.app", "io.github.other": "\(current)/Other.app"])
    #expect(try Command.appCopies([], current: current).isEmpty)

    guard case .run(let request) = try Command.parse(["run", "t", "--app-copy", "a=\(current)/Copy.app", "--app-copy=b=\(current)/Other.app"])
    else { throw UsageError(message: "not a run") }
    #expect(request.appCopies == ["a": "\(current)/Copy.app", "b": "\(current)/Other.app"])
    #expect(Help.text("run").contains("--app-copy <id>=<path>"))
  }

  @Test func theFlagRefusesWhatIsNotACopyOfAnApp() throws {
    let folder = try scratch()
    defer { try? FileManager.default.removeItem(at: folder) }
    let current = folder.path(percentEncoded: false)
    func message(_ values: [String]) -> String? {
      do {
        _ = try Command.appCopies(values, current: current)
        return nil
      } catch {
        return error.message
      }
    }
    #expect(message(["Copy.app"])?.hasPrefix("--app-copy needs an app id and the path of its copy") == true)
    #expect(message(["=Copy.app"])?.hasSuffix("not “=Copy.app”") == true)
    #expect(message(["a="])?.hasSuffix("not “a=”") == true)
    #expect(message(["a=Copy.app", "a=Other.app"]) == "--app-copy names a copy of a twice")
    #expect(message(["a=Copy"]) == "--app-copy needs the path of an app, a folder ending in .app, not “Copy”")
    #expect(message(["a=Gone.app"]) == "there is no app at \(current)/Gone.app for --app-copy")
    #expect(throws: UsageError.self) { try Command.parse(["run", "t", "--app-copy"]) }
  }

  @Test func aCopyStandsForItsAppAndOnlyItsProcessesAreTheApp() throws {
    let folder = try scratch()
    defer { try? FileManager.default.removeItem(at: folder) }
    let app = "test.copy.\(UUID().uuidString)"
    let copy = folder.appending(path: "Copy.app", directoryHint: .isDirectory)
    #expect(AppCopies.copy(of: app) == nil)
    AppCopies.use([app: copy])
    defer { AppCopies.use([:]) }
    #expect(AppCopies.copy(of: app) == copy)
    #expect(AppCopies.copy(of: "com.example.Other") == nil)

    let installed = URL(filePath: "/Applications/Example.app/")
    // Without a copy, every process with the app's id is the app.
    #expect(AppCopies.isApp(app, bundle: installed, identifier: app, copy: nil))
    #expect(!AppCopies.isApp(app, bundle: installed, identifier: "com.example.Other", copy: nil))
    // With one, only those started from the copy, however its path is written.
    #expect(!AppCopies.isApp(app, bundle: installed, identifier: app, copy: copy))
    #expect(AppCopies.isApp(app, bundle: copy, identifier: app, copy: copy))
    let other = folder.appending(path: "Other.app/../Copy.app/")
    #expect(AppCopies.isApp(app, bundle: other, identifier: "renamed.copy", copy: copy))
    #expect(!AppCopies.isApp(app, bundle: nil, identifier: app, copy: copy))
    // A process names the copy by its real path, without the links the run was given.
    let link = folder.appending(path: "Link.app")
    try FileManager.default.createSymbolicLink(at: link, withDestinationURL: copy)
    #expect(AppCopies.isApp(app, bundle: copy.resolvingSymlinksInPath(), identifier: app, copy: link))
    #expect(MacHands.runningApps(app).isEmpty)
  }

  @Test func aCopyIsLookedForByItsOwnIdToo() throws {
    let folder = try scratch()
    defer { try? FileManager.default.removeItem(at: folder) }
    let copy = folder.appending(path: "Copy.app", directoryHint: .isDirectory)
    #expect(AppCopies.identifiers(of: "com.example.App", copy: copy) == ["com.example.App"])
    let contents = copy.appending(path: "Contents", directoryHint: .isDirectory)
    try FileManager.default.createDirectory(at: contents, withIntermediateDirectories: true)
    let info = contents.appending(path: "Info.plist")
    try (["CFBundleIdentifier": "com.example.App"] as NSDictionary).write(to: info)
    #expect(AppCopies.identifiers(of: "com.example.App", copy: copy) == ["com.example.App"])
    try (["CFBundleIdentifier": "renamed.copy"] as NSDictionary).write(to: info)
    #expect(AppCopies.identifiers(of: "com.example.App", copy: copy) == ["com.example.App", "renamed.copy"])
  }
}
