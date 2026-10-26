import Darwin
import Foundation
import JEVisionHands
import Observation

/// Observation carries an invalidation only. Every binding still reads its selected backend.
@MainActor
final class PreferenceContext: Observable {
  static let current = PreferenceContext(Profile.current.defaults)
  let store: PreferenceStore
  private let registrar = ObservationRegistrar()
  private var serial: UInt64 = 0
  private var revision: UInt64 {
    registrar.access(self, keyPath: \.revision)
    return serial
  }
  private let lifetime = PreferenceObservationLifetime()
  private var started = false
  private var usable = true
  private var pendingEvent = false
  private var pendingNativeRefresh = false
  private var observedFile: URL?
  private var stamp: PreferenceFileStamp?
  private var nativeKeys: Set<String> = []
  private var lastFailure: AppPreferences.Failure?

  init(_ source: any PreferenceSource) { store = source.preferenceStore }

  func read<Value>(_ key: String, fallback: Value, decode: (PreferenceStore, String) -> Value) -> Value {
    start()
    observeNative(key)
    _ = revision
    guard usable else { return fallback }
    return decode(store, key)
  }

  private func start() {
    guard !started else { return }
    started = true
    let selected = store
    lifetime.add(NotificationCenter.default.addObserver(forName: PreferenceStore.changed, object: nil, queue: nil) {
      [weak self] note in
      guard let changed = note.object as? PreferenceStore,
        changed.identity == selected.identity ||
          (selected.fileURL != nil && changed.fileURL == selected.fileURL) else { return }
      Task { @MainActor in self?.scheduleRefresh() }
    })
    if let defaults = store.nativeDefaults {
      // KVO key paths cannot represent a literal dotted preference name. Foundation's selected
      // defaults notification also covers such keys, registration changes and domain replacement.
      lifetime.add(NotificationCenter.default.addObserver(forName: UserDefaults.didChangeNotification,
        object: defaults, queue: nil) { [weak self] _ in
        Task { @MainActor in self?.scheduleRefresh() }
      })
    }
    if let file = AppPreferences.storedFile(for: store) {
      observedFile = file
      lifetime.file = PreferenceFileObservation(file: file) { [weak self] in
        Task { @MainActor in self?.scheduleRefresh(fromFile: true) }
      }
      stamp = PreferenceFileStamp(file)
    }
    usable = validate()
  }

  private func observeNative(_ key: String) {
    guard let defaults = store.nativeDefaults, nativeKeys.insert(key).inserted,
      !key.contains(".") else { return }
    lifetime.keys.append(PreferenceNativeKeyObservation(defaults, key: key) { [weak self] in
      Task { @MainActor in self?.scheduleRefresh() }
    })
  }

  private func scheduleRefresh(fromFile: Bool = false) {
    pendingNativeRefresh = pendingNativeRefresh || fromFile
    guard !pendingEvent else { return }
    pendingEvent = true
    Task { @MainActor [weak self] in
      // Coalesce KVO, a store commit and the matching directory event on the same turn.
      await Task.yield()
      guard let self else { return }
      pendingEvent = false
      let refreshNative = pendingNativeRefresh
      pendingNativeRefresh = false
      refresh(native: refreshNative)
    }
  }

  private func refresh(native: Bool) {
    let previousUsable = usable
    usable = validate(refreshNative: native)
    guard usable else { return }
    if let file = observedFile {
      let newest = PreferenceFileStamp(file)
      guard newest != stamp || !previousUsable else { return }
      stamp = newest
    }
    registrar.withMutation(of: self, keyPath: \.revision) { serial &+= 1 }
  }

  private func validate(refreshNative: Bool = false) -> Bool {
    do {
      try AppPreferences.check(store)
      if store.fileURL != nil || refreshNative, !store.refresh() {
        throw AppPreferences.Failure(kind: .unreadable)
      }
      lastFailure = nil
      return true
    } catch {
      let failure = error as? AppPreferences.Failure ?? AppPreferences.Failure(kind: .unreadable)
      if failure != lastFailure {
        lastFailure = failure
        NotificationCenter.default.post(name: AppPreferences.refusedWrite, object: store,
          userInfo: [AppPreferences.failureKey: failure])
      }
      return false
    }
  }
}

/// Owns non-Sendable Foundation observation tokens without transferring them across actors.
private final class PreferenceObservationLifetime: @unchecked Sendable {
  private var notifications: [any NSObjectProtocol] = []
  var keys: [PreferenceNativeKeyObservation] = []
  var file: PreferenceFileObservation?
  func add(_ token: any NSObjectProtocol) { notifications.append(token) }
  deinit { for token in notifications { NotificationCenter.default.removeObserver(token) } }
}

private final class PreferenceNativeKeyObservation: NSObject, @unchecked Sendable {
  private let defaults: UserDefaults
  private let key: String
  private let changed: @Sendable () -> Void
  init(_ defaults: UserDefaults, key: String, changed: @escaping @Sendable () -> Void) {
    self.defaults = defaults
    self.key = key
    self.changed = changed
    super.init()
    defaults.addObserver(self, forKeyPath: key, options: [.old, .new], context: nil)
  }
  override func observeValue(forKeyPath keyPath: String?, of object: Any?,
    change: [NSKeyValueChangeKey: Any]?, context: UnsafeMutableRawPointer?) {
    if let old = change?[.oldKey] as? NSObject, let new = change?[.newKey] as? NSObject,
      old.isEqual(new) { return }
    changed()
  }
  deinit { defaults.removeObserver(self, forKeyPath: key) }
}

/// File identity and timestamps deduplicate directory echoes; preference values are never cached.
private struct PreferenceFileStamp: Equatable, Sendable {
  let device: Int32
  let inode: UInt64
  let size: Int64
  let modifiedSeconds: Int
  let modifiedNanoseconds: Int
  let changedSeconds: Int
  let changedNanoseconds: Int
  let mode: UInt16
  init?(_ url: URL) {
    var info = stat()
    guard lstat(url.path, &info) == 0 else { return nil }
    device = info.st_dev
    inode = info.st_ino
    size = info.st_size
    modifiedSeconds = info.st_mtimespec.tv_sec
    modifiedNanoseconds = info.st_mtimespec.tv_nsec
    changedSeconds = info.st_ctimespec.tv_sec
    changedNanoseconds = info.st_ctimespec.tv_nsec
    mode = info.st_mode
  }
}

/// The directory watch survives atomic file replacement. The file watch follows its new inode
/// and also catches in-place writes, permission changes, deletion and subsequent recreation.
private final class PreferenceFileObservation: @unchecked Sendable {
  private let lock = NSLock()
  private let queue = DispatchQueue(label: "JEVision.preferenceFileObservation")
  private let file: URL
  private let changed: @Sendable () -> Void
  private var directorySource: (any DispatchSourceFileSystemObject)?
  private var ancestorSource: (any DispatchSourceFileSystemObject)?
  private var fileSource: (any DispatchSourceFileSystemObject)?
  private var directoryStamp: PreferenceFileStamp?
  private var watchedDirectory: URL?
  private var fileStamp: PreferenceFileStamp?

  init(file: URL, changed: @escaping @Sendable () -> Void) {
    self.file = file
    self.changed = changed
    lock.withLock {
      // Keep an already-armed ancestor while the profile's own directory watch is replaced.
      var ancestor = file.deletingLastPathComponent().deletingLastPathComponent()
      while (PreferenceFileStamp(ancestor)?.mode ?? 0) & UInt16(S_IFMT) != UInt16(S_IFDIR), ancestor.path != "/" {
        ancestor.deleteLastPathComponent()
      }
      ancestorSource = source(ancestor)
      rearm()
    }
  }

  private func source(_ url: URL) -> (any DispatchSourceFileSystemObject)? {
    let fd = Darwin.open(url.path, O_EVTONLY | O_CLOEXEC | O_NOFOLLOW)
    guard fd >= 0 else { return nil }
    let result = DispatchSource.makeFileSystemObjectSource(fileDescriptor: fd,
      eventMask: [.write, .extend, .rename, .delete, .attrib, .revoke], queue: queue)
    result.setEventHandler { [weak self] in self?.event() }
    // Recheck after kernel registration to close the gap while a replaced directory is rearmed.
    result.setRegistrationHandler { [weak self] in self?.event() }
    result.setCancelHandler { Darwin.close(fd) }
    result.resume()
    return result
  }

  private func rearm() {
    // If the whole profile directory is removed, its surviving ancestor observes recreation.
    var parent = file.deletingLastPathComponent()
    while (PreferenceFileStamp(parent)?.mode ?? 0) & UInt16(S_IFMT) != UInt16(S_IFDIR), parent.path != "/" {
      parent.deleteLastPathComponent()
    }
    let newestDirectory = PreferenceFileStamp(parent)
    if directorySource == nil || watchedDirectory != parent || newestDirectory?.inode != directoryStamp?.inode ||
      newestDirectory?.device != directoryStamp?.device {
      directorySource?.cancel()
      directorySource = source(parent)
      directoryStamp = newestDirectory
      watchedDirectory = parent
    }
    let newestFile = PreferenceFileStamp(file)
    if fileSource == nil || newestFile?.inode != fileStamp?.inode || newestFile?.device != fileStamp?.device {
      fileSource?.cancel()
      fileSource = newestFile == nil ? nil : source(file)
    }
    fileStamp = newestFile
  }

  private func event() {
    let differs = lock.withLock {
      let previous = fileStamp
      rearm()
      return previous != fileStamp
    }
    if differs { changed() }
  }

  deinit {
    ancestorSource?.cancel()
    directorySource?.cancel()
    fileSource?.cancel()
  }
}
