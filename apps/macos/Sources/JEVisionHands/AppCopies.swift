import AppKit
import Synchronization

/// Copies of apps a run uses instead of the installed ones, from `jevision run --app-copy ID=PATH`.
/// A test that names the app `ID` opens, watches, records and quits only the processes started
/// from the copy, so it can drive a second copy of an app the person has open, JEVision itself
/// included.
public enum AppCopies {
  private static let copies = Mutex<[String: URL]>([:])

  /// Makes runs use `copies`, by the app each one stands for, instead of the installed apps.
  public static func use(_ copies: [String: URL]) {
    self.copies.withLock { $0 = copies }
  }

  /// The copy that stands for `app`, if there is one.
  public static func copy(of app: String) -> URL? {
    copies.withLock { $0[app] }
  }

  /// The ids a process started from `copy` can have: `app`'s, and the copy's own when it was
  /// given another.
  static func identifiers(of app: String, copy: URL) -> [String] {
    let own = NSDictionary(contentsOf: copy.appending(path: "Contents/Info.plist"))?["CFBundleIdentifier"] as? String
    return own.map { $0 == app ? [app] : [app, $0] } ?? [app]
  }

  /// Whether a process started from `bundle` (with the id `identifier`) is the app `app`: one
  /// started from its copy when it has one, otherwise any with its id.
  static func isApp(_ app: String, bundle: URL?, identifier: String?, copy: URL?) -> Bool {
    guard let copy else { return identifier == app }
    return bundle.map { path($0) == path(copy) } ?? false
  }

  /// `url`'s path with links resolved, the way the process that runs it sees it.
  static func path(_ url: URL) -> String {
    let given = url.standardizedFileURL.path(percentEncoded: false)
    guard let real = realpath(given, nil) else { return given.hasSuffix("/") ? String(given.dropLast()) : given }
    defer { free(real) }
    return String(cString: real)
  }
}
