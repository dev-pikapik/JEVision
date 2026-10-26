import CoreFoundation
import Darwin
import Foundation

/// A preferences source can be native defaults or an isolated profile's own file.
public protocol PreferenceSource: AnyObject {
  var preferenceStore: PreferenceStore { get }
}

extension UserDefaults: PreferenceSource {
  public var preferenceStore: PreferenceStore { PreferenceStore(native: self) }
}

/// Native preferences retain Foundation's behavior. Isolated preferences read their file afresh,
/// without the forced-preferences search layer of a path-named UserDefaults suite.
public final class PreferenceStore: PreferenceSource, @unchecked Sendable {
  public static let standard = PreferenceStore(native: .standard)
  public static let changed = Notification.Name("JEVision.preferenceStoreChanged")
  public var preferenceStore: PreferenceStore { self }
  public let nativeDefaults: UserDefaults?
  public let fileURL: URL?
  public var identity: ObjectIdentifier { nativeDefaults.map(ObjectIdentifier.init) ?? ObjectIdentifier(self) }

  private let lock = NSRecursiveLock()
  private var registration: [String: Any] = [:]
  private var volatile: [String: [String: Any]] = [:]
  private var pending: Edits?
  private var failure: Error?
  private var pendingFailure: Error?

  private struct Edits {
    var values: [String: Any] = [:]
    var removed: Set<String> = []
    func applying(to original: [String: Any]) -> [String: Any] {
      var result = original
      for key in removed { result.removeValue(forKey: key) }
      result.merge(values) { _, new in new }
      return result
    }
  }

  public init(native: UserDefaults) { nativeDefaults = native; fileURL = nil }
  public init(file: URL) { nativeDefaults = nil; fileURL = file.standardizedFileURL }

  private var domain: String? {
    fileURL.map { $0.path.hasSuffix(".plist") ? String($0.path.dropLast(6)) : $0.path }
  }

  public func object(forKey key: String) -> Any? {
    if let nativeDefaults { return nativeDefaults.object(forKey: key) }
    return lock.withLock { volatile[UserDefaults.argumentDomain]?[key] ?? (try? values())?[key] ?? registration[key] }
  }

  public func bool(forKey key: String) -> Bool {
    if let nativeDefaults { return nativeDefaults.bool(forKey: key) }
    if let value = object(forKey: key) as? NSNumber { return value.boolValue }
    return (object(forKey: key) as? NSString)?.boolValue ?? false
  }

  public func integer(forKey key: String) -> Int {
    if let nativeDefaults { return nativeDefaults.integer(forKey: key) }
    if let value = object(forKey: key) as? NSNumber { return value.intValue }
    return (object(forKey: key) as? NSString)?.integerValue ?? 0
  }

  public func double(forKey key: String) -> Double {
    if let nativeDefaults { return nativeDefaults.double(forKey: key) }
    if let value = object(forKey: key) as? NSNumber { return value.doubleValue }
    return (object(forKey: key) as? NSString)?.doubleValue ?? 0
  }

  public func float(forKey key: String) -> Float {
    if let nativeDefaults { return nativeDefaults.float(forKey: key) }
    return Float(double(forKey: key))
  }

  public func string(forKey key: String) -> String? {
    if let nativeDefaults { return nativeDefaults.string(forKey: key) }
    if let value = object(forKey: key) as? String { return value }
    return (object(forKey: key) as? NSNumber)?.stringValue
  }

  public func data(forKey key: String) -> Data? {
    nativeDefaults.map { $0.data(forKey: key) } ?? (object(forKey: key) as? Data)
  }
  public func array(forKey key: String) -> [Any]? {
    nativeDefaults.map { $0.array(forKey: key) } ?? (object(forKey: key) as? [Any])
  }
  public func dictionary(forKey key: String) -> [String: Any]? {
    nativeDefaults.map { $0.dictionary(forKey: key) } ?? (object(forKey: key) as? [String: Any])
  }
  public func stringArray(forKey key: String) -> [String]? {
    nativeDefaults.map { $0.stringArray(forKey: key) } ?? (object(forKey: key) as? [String])
  }

  public func url(forKey key: String) -> URL? {
    if let nativeDefaults { return nativeDefaults.url(forKey: key) }
    if let path = object(forKey: key) as? String {
      return URL(filePath: (path as NSString).expandingTildeInPath)
    }
    guard let data = object(forKey: key) as? Data else { return nil }
    return (try? NSKeyedUnarchiver.unarchivedObject(ofClass: NSURL.self, from: data)) as URL?
  }

  public func dictionaryRepresentation() -> [String: Any] {
    if let nativeDefaults { return nativeDefaults.dictionaryRepresentation() }
    return lock.withLock {
      registration.merging((try? values()) ?? [:]) { _, saved in saved }
        .merging(volatile[UserDefaults.argumentDomain] ?? [:]) { _, argument in argument }
    }
  }

  public func persistentDomain(forName name: String) -> [String: Any]? {
    if let nativeDefaults { return nativeDefaults.persistentDomain(forName: name) }
    guard name == domain else { return nil }
    return lock.withLock { try? values() }
  }

  public func register(defaults values: [String: Any]) {
    if let nativeDefaults { nativeDefaults.register(defaults: values); return }
    lock.withLock { registration.merge(values) { _, new in new } }
  }

  public func volatileDomain(forName name: String) -> [String: Any] {
    if let nativeDefaults { return nativeDefaults.volatileDomain(forName: name) }
    return lock.withLock { volatile[name] ?? [:] }
  }

  public func setVolatileDomain(_ values: [String: Any], forName name: String) {
    if let nativeDefaults { nativeDefaults.setVolatileDomain(values, forName: name); return }
    lock.withLock { volatile[name] = values }
  }

  public func set(_ value: Any?, forKey key: String) {
    if let nativeDefaults {
      if let url = value as? URL { nativeDefaults.set(url, forKey: key) }
      else { nativeDefaults.set(value, forKey: key) }
      notify()
      return
    }
    mutate {
      guard let value else { pending?.values.removeValue(forKey: key); pending?.removed.insert(key); return }
      pending?.values[key] = try Self.encoded(value)
      pending?.removed.remove(key)
    }
  }

  public func removeObject(forKey key: String) { set(nil, forKey: key) }

  public func setPersistentDomain(_ values: [String: Any], forName name: String) {
    if let nativeDefaults { nativeDefaults.setPersistentDomain(values, forName: name); notify(); return }
    guard name == domain else { return }
    mutate {
      for key in try self.values().keys { pending?.removed.insert(key) }
      pending?.values = try values.mapValues(Self.encoded)
      for key in values.keys { pending?.removed.remove(key) }
    }
  }

  public func removePersistentDomain(forName name: String) {
    if let nativeDefaults { nativeDefaults.removePersistentDomain(forName: name); notify(); return }
    setPersistentDomain([:], forName: name)
  }

  @discardableResult public func synchronize() -> Bool {
    if let nativeDefaults { return nativeDefaults.synchronize() }
    return lock.withLock { pending != nil ? pendingFailure == nil : failure == nil && (try? readFile()) != nil }
  }

  @discardableResult public func refresh() -> Bool {
    if let nativeDefaults { return nativeDefaults.synchronize() }
    return lock.withLock { (try? readFile()) != nil }
  }

  /// Profile edits are short-lived: re-read and validate while holding a shared-process lock,
  /// stage only edited keys, then merge into the newest validated file before replacing it.
  public func transaction(validate: ([String: Any]) throws -> Void, _ body: () throws -> Void) throws {
    if let nativeDefaults {
      // Native callers know their domain and must validate its stored values, not this merged list.
      try lock.withLock { try validate(nativeDefaults.dictionaryRepresentation()); try body() }
      return
    }
    var changed = false
    do {
      try lock.withLock {
        if pending != nil { try validate(readFile()); try body(); return }
        guard let fileURL else { return }
        try FileManager.default.createDirectory(at: fileURL.deletingLastPathComponent(), withIntermediateDirectories: true,
          attributes: [.posixPermissions: 0o700])
        let fileLock = try AdvisoryLock(file: fileURL.appendingPathExtension("lock"))
        defer { fileLock.close() }
        try validate(readFile())
        pending = Edits()
        pendingFailure = nil
        defer { pending = nil; pendingFailure = nil }
        try body()
        if let pendingFailure { throw pendingFailure }
        let newest = try readFile()
        try validate(newest)
        let edited = pending!.applying(to: newest)
        if !Self.equal(edited, newest) {
          try Self.replace(edited, at: fileURL)
          changed = true
        }
        failure = nil
      }
    } catch {
      lock.withLock { failure = error }
      throw error
    }
    if changed { notify() }
  }

  private func mutate(_ body: () throws -> Void) {
    do {
      if lock.withLock({ pending != nil }) { try lock.withLock(body) }
      else { try transaction(validate: { _ in }, body) }
    } catch { lock.withLock { failure = error; if pending != nil { pendingFailure = error } } }
  }

  private func values() throws -> [String: Any] {
    let saved = try readFile()
    return pending?.applying(to: saved) ?? saved
  }

  private func readFile() throws -> [String: Any] {
    guard let fileURL else { return [:] }
    let fd = Darwin.open(fileURL.path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW)
    guard fd >= 0 else {
      if errno == ENOENT { return [:] }
      throw Self.posix()
    }
    let handle = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
    var info = stat()
    guard fstat(fd, &info) == 0 else { throw Self.posix() }
    guard info.st_mode & S_IFMT == S_IFREG else { throw CocoaError(.fileReadUnsupportedScheme) }
    let data = try handle.readToEnd() ?? Data()
    guard let values = try PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any]
    else { throw CocoaError(.propertyListReadCorrupt) }
    return values
  }

  private static func equal(_ first: Any, _ second: Any) -> Bool {
    if let first = first as? [String: Any], let second = second as? [String: Any] {
      return first.count == second.count && first.allSatisfy { key, value in
        second[key].map { equal(value, $0) } ?? false
      }
    }
    if let first = first as? [Any], let second = second as? [Any] {
      return first.count == second.count && zip(first, second).allSatisfy(equal)
    }
    if let first = first as? NSNumber, let second = second as? NSNumber {
      let firstBoolean = CFGetTypeID(first) == CFBooleanGetTypeID()
      let secondBoolean = CFGetTypeID(second) == CFBooleanGetTypeID()
      let firstReal = ["f", "d"].contains(String(cString: first.objCType))
      let secondReal = ["f", "d"].contains(String(cString: second.objCType))
      return firstBoolean == secondBoolean && firstReal == secondReal && first == second
    }
    return (first as? NSObject)?.isEqual(second) ?? false
  }

  private static func encoded(_ value: Any) throws -> Any {
    guard let url = value as? URL else { return value }
    if url.isFileURL { return (url.path as NSString).abbreviatingWithTildeInPath }
    return try NSKeyedArchiver.archivedData(withRootObject: url as NSURL, requiringSecureCoding: true)
  }

  private static func replace(_ values: [String: Any], at file: URL) throws {
    let data = try PropertyListSerialization.data(fromPropertyList: values, format: .binary, options: 0)
    let temporary = file.deletingLastPathComponent().appending(path: "." + file.lastPathComponent + "." + UUID().uuidString)
    let fd = Darwin.open(temporary.path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, 0o600)
    guard fd >= 0 else { throw posix() }
    var closed = false
    defer { if !closed { Darwin.close(fd) }; try? FileManager.default.removeItem(at: temporary) }
    try data.withUnsafeBytes { bytes in
      var offset = 0
      while offset < bytes.count {
        let count = Darwin.write(fd, bytes.baseAddress!.advanced(by: offset), bytes.count - offset)
        if count < 0 { if errno == EINTR { continue }; throw posix() }
        guard count > 0 else { throw POSIXError(.EIO) }
        offset += count
      }
    }
    guard fsync(fd) == 0 else { throw posix() }
    let closeResult = Darwin.close(fd)
    closed = true
    guard closeResult == 0 else { throw posix() }
    guard Darwin.rename(temporary.path, file.path) == 0 else { throw posix() }
  }

  private static func posix() -> POSIXError { POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
  private func notify() { NotificationCenter.default.post(name: Self.changed, object: self) }

  private final class AdvisoryLock {
    let fd: Int32
    init(file: URL) throws {
      fd = Darwin.open(file.path, O_RDWR | O_CREAT | O_CLOEXEC | O_NOFOLLOW, 0o600)
      guard fd >= 0 else { throw PreferenceStore.posix() }
      var info = stat()
      guard fstat(fd, &info) == 0, info.st_mode & S_IFMT == S_IFREG, info.st_uid == geteuid(), fchmod(fd, 0o600) == 0 else {
        let error = PreferenceStore.posix(); Darwin.close(fd); throw error
      }
      while flock(fd, LOCK_EX) != 0 {
        if errno == EINTR { continue }
        let error = PreferenceStore.posix(); Darwin.close(fd); throw error
      }
    }
    func close() { _ = flock(fd, LOCK_UN); Darwin.close(fd) }
  }
}
