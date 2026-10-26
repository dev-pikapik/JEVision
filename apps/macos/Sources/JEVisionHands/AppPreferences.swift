import CoreFoundation
import Foundation
import Synchronization

/// Compatibility of the app's preferences, shared by the app and its command-line helper.
/// Native settings stay in UserDefaults; profile writes retain their local file and unfamiliar keys.
public enum AppPreferences {
  public static let formatKey = "preferences.format"
  public static let format = 1
  public static let refusedWrite = Notification.Name("JEVision.preferencesRefusedWrite")
  public static let failureKey = "failure"

  public struct Failure: LocalizedError, Equatable, Sendable {
    public enum Kind: Sendable { case newer, unreadable, backup }
    public let kind: Kind
    public init(kind: Kind) { self.kind = kind }
    public var status: Int32 { kind == .newer ? 5 : 3 }
    public var code: String {
      switch kind {
      case .newer: "preferences.needsNewer"
      case .unreadable: "preferences.unreadable"
      case .backup: "preferences.backupFailed"
      }
    }
    public var errorDescription: String? {
      switch kind {
      case .newer: handsWords("preferences.needsNewer")
      case .unreadable: handsWords("preferences.unreadable")
      case .backup: handsWords("preferences.backupFailed")
      }
    }
  }

  private final class Store: @unchecked Sendable {
    let defaults: PreferenceStore
    let domain: String
    let file: URL
    let backups: URL
    let lock = NSRecursiveLock()

    init(defaults: PreferenceStore, domain: String, file: URL, backups: URL) {
      self.defaults = defaults
      self.domain = domain
      self.file = file
      self.backups = backups
    }
  }

  private static let stores = Mutex<[ObjectIdentifier: Store]>([:])

  /// Register a concrete domain, without reading it or causing a migration. Injectable stores
  /// use the same path as Profile, so tests never have to inspect the person's preferences.
  public static func register(_ source: any PreferenceSource, domain: String, file: URL, backups: URL) {
    let defaults = source.preferenceStore
    stores.withLock { $0[defaults.identity] = Store(defaults: defaults, domain: domain, file: file, backups: backups) }
  }

  private static func store(_ source: any PreferenceSource) -> Store? {
    stores.withLock { $0[source.preferenceStore.identity] }
  }

  /// The registered native file also provides change observation for literal dotted keys.
  public static func storedFile(for source: any PreferenceSource) -> URL? {
    store(source)?.file ?? source.preferenceStore.fileURL
  }

  /// Read-only: legacy preferences are supported, but are never stamped by a CLI command.
  public static func checkApp() throws {
    try check(Profile.current.appDefaults)
  }

  public static func check(_ source: any PreferenceSource, domain: String? = nil) throws {
    let defaults = source.preferenceStore
    if let store = store(defaults) {
      try store.lock.withLock { _ = try inspect(store) }
    } else if let file = defaults.fileURL {
      _ = try header(try disk(file).values)
    } else if let domain {
      _ = try header(defaults.persistentDomain(forName: domain) ?? [:])
    }
  }

  /// Before any GUI initializer may read or write settings: keep a private legacy snapshot,
  /// then stamp one key only. A failed backup leaves the original domain alone.
  @discardableResult
  public static func prepareForGUI(_ profile: Profile = .current) throws -> URL? {
    guard let store = store(profile.defaults) else { return nil }
    return try store.lock.withLock {
      let snapshot = try inspect(store)
      guard !snapshot.current else { return nil }
      var backup: URL?
      if snapshot.raw != nil || !snapshot.values.isEmpty {
        let target = store.backups.appending(path: "legacy-" + UUID().uuidString, directoryHint: .isDirectory)
        do {
          let files = FileManager.default
          try files.createDirectory(at: store.backups, withIntermediateDirectories: true,
            attributes: [.posixPermissions: 0o700])
          try files.setAttributes([.posixPermissions: 0o700], ofItemAtPath: store.backups.path)
          try files.createDirectory(at: target, withIntermediateDirectories: false,
            attributes: [.posixPermissions: 0o700])
          let data = try PropertyListSerialization.data(fromPropertyList: snapshot.values, format: .binary, options: 0)
          let saved = target.appending(path: "Settings.plist")
          try data.write(to: saved, options: .atomic)
          try files.setAttributes([.posixPermissions: 0o600], ofItemAtPath: saved.path)
          if let raw = snapshot.raw {
            let original = target.appending(path: "Settings.original.plist")
            try raw.write(to: original, options: .atomic)
            try files.setAttributes([.posixPermissions: 0o600], ofItemAtPath: original.path)
          }
          backup = target
        } catch {
          try? FileManager.default.removeItem(at: target)
          throw Failure(kind: .backup)
        }
      }
      // Recheck after the backup: another process may have updated the domain meanwhile.
      _ = try inspect(store)
      do {
        try FileManager.default.createDirectory(at: store.file.deletingLastPathComponent(), withIntermediateDirectories: true)
      } catch { throw Failure(kind: .unreadable) }
      do {
        try store.defaults.transaction(validate: { _ in _ = try inspect(store) }) {
          // Native defaults may still hold an identical marker from an externally restored file.
          if snapshot.values[formatKey] != nil {
            store.defaults.removeObject(forKey: formatKey)
            guard store.defaults.synchronize() else { throw Failure(kind: .unreadable) }
          }
          store.defaults.set(format, forKey: formatKey)
        }
        guard store.defaults.synchronize() else { throw Failure(kind: .unreadable) }
      } catch let failure as Failure {
        throw failure
      } catch {
        throw Failure(kind: .unreadable)
      }
      return backup
    }
  }

  /// All application writers, including SwiftUI bindings, pass through this operation.
  /// Refusal is visible to the GUI through one shared error notification; throwing callers
  /// (reset and command entry points) can present or return the error themselves.
  @discardableResult
  public static func write(in source: any PreferenceSource, _ body: () throws -> Void) -> Bool {
    let defaults = source.preferenceStore
    do {
      try checkedWrite(in: defaults, body)
      return true
    } catch let failure as Failure {
      NotificationCenter.default.post(name: refusedWrite, object: defaults, userInfo: [failureKey: failure])
      return false
    } catch {
      return false
    }
  }

  public static func checkedWrite(in source: any PreferenceSource, domain: String? = nil, _ body: () throws -> Void) throws {
    let defaults = source.preferenceStore
    do {
      if let store = store(defaults) {
        try store.lock.withLock {
          try defaults.transaction(validate: { _ in _ = try inspect(store) }, body)
        }
      } else {
        try defaults.transaction(validate: { _ in try check(defaults, domain: domain) }, body)
      }
    } catch let failure as Failure {
      throw failure
    } catch {
      throw Failure(kind: .unreadable)
    }
  }

  @discardableResult
  public static func set(_ value: Any?, forKey key: String, in source: any PreferenceSource) -> Bool {
    let defaults = source.preferenceStore
    return write(in: defaults) { defaults.set(value, forKey: key) }
  }

  @discardableResult
  public static func removeObject(forKey key: String, in source: any PreferenceSource) -> Bool {
    let defaults = source.preferenceStore
    return write(in: defaults) { defaults.removeObject(forKey: key) }
  }

  private struct Snapshot {
    let current: Bool
    let values: [String: Any]
    let raw: Data?
  }

  private static func inspect(_ store: Store) throws -> Snapshot {
    let stored = try disk(store.file)
    let raw = stored.raw
    let disk = stored.values
    let diskCurrent = try header(disk)
    let values = store.defaults.persistentDomain(forName: store.domain) ?? [:]
    let memoryCurrent = try header(values)
    // A replaced legacy file needs its own backup even when this process cached an older marker.
    return Snapshot(current: raw == nil ? memoryCurrent : diskCurrent, values: values, raw: raw)
  }

  private static func disk(_ file: URL) throws -> (raw: Data?, values: [String: Any]) {
    var raw: Data?
    var disk: [String: Any] = [:]
    // UserDefaults may turn an unreadable plist into an empty domain. Inspect bytes first,
    // without ever writing the underlying defaults file directly.
    do {
      raw = try Data(contentsOf: file)
    } catch let error as CocoaError where error.code == .fileReadNoSuchFile {
    } catch {
      throw Failure(kind: .unreadable)
    }
    if let raw {
      do {
        guard let contents = try PropertyListSerialization.propertyList(from: raw, options: [], format: nil) as? [String: Any]
        else { throw Failure(kind: .unreadable) }
        disk = contents
      } catch {
        throw Failure(kind: .unreadable)
      }
    }
    return (raw, disk)
  }

  /// Inspect only the stored header. Argument, global and registration domains cannot opt an
  /// unsupported file into being writable. Reject real 1.0 as well as booleans and strings.
  private static func header(_ values: [String: Any]) throws -> Bool {
    guard let value = values[formatKey] else { return false }
    guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(),
      !["f", "d"].contains(String(cString: number.objCType)), number.compare(0) == .orderedDescending
    else { throw Failure(kind: .unreadable) }
    guard number.compare(NSNumber(value: format)) != .orderedDescending else { throw Failure(kind: .newer) }
    guard number.compare(NSNumber(value: format)) == .orderedSame else { throw Failure(kind: .unreadable) }
    return true
  }
}
