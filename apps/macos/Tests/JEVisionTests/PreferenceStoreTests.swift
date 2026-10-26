import CoreFoundation
import Foundation
import JEVisionHands
import Synchronization
import Testing

private struct FilePreferencesFixture {
  let folder: URL
  let file: URL
  let store: PreferenceStore
  var domain: String { String(file.path.dropLast(6)) }

  init(values: [String: Any] = ["preferences.format": 1, "shown": false]) throws {
    folder = FileManager.default.temporaryDirectory.appending(path: "jevision-file-preferences-" + UUID().uuidString)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    file = folder.appending(path: "Settings.plist")
    try Self.write(values, to: file)
    store = PreferenceStore(file: file)
  }

  static func write(_ values: [String: Any], to file: URL) throws {
    try PropertyListSerialization.data(fromPropertyList: values, format: .xml, options: 0).write(to: file, options: .atomic)
  }

  func saved() throws -> [String: Any] {
    try #require(PropertyListSerialization.propertyList(from: Data(contentsOf: file), format: nil) as? [String: Any])
  }

  func discard() {
    try? FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: folder.path)
    try? FileManager.default.removeItem(at: folder)
  }
}

struct PreferenceStoreTests {
  private enum Refusal: Error { case future, stopped }

  @Test func persistedValuesChangeAtOnceAndRetainUnknownPropertyListTypes() throws {
    let unknown: [String: Any] = ["data": Data([2, 4]), "date": Date(timeIntervalSince1970: 12345),
      "nested": ["list": [true, 9, "kept"]]]
    let fixture = try FilePreferencesFixture(values: ["preferences.format": 1, "shown": false, "number": 2,
      "text": "before", "bytes": Data([0]), "unknown": unknown])
    defer { fixture.discard() }
    let store = fixture.store
    #expect(!store.bool(forKey: "shown"))
    store.set(true, forKey: "shown")
    store.set(4, forKey: "number")
    store.set("after", forKey: "text")
    store.set(Data([1, 3]), forKey: "bytes")
    #expect(store.synchronize())
    #expect(store.bool(forKey: "shown"))
    #expect(store.integer(forKey: "number") == 4)
    #expect(store.string(forKey: "text") == "after")
    #expect(store.data(forKey: "bytes") == Data([1, 3]))
    #expect(try fixture.saved()["unknown"] as? NSDictionary == unknown as NSDictionary)
    #expect(store.persistentDomain(forName: fixture.domain)?["preferences.format"] as? Int == 1)
    #expect(store.persistentDomain(forName: "other") == nil)
    #expect(store.nativeDefaults == nil)
  }

  @Test func changingANumericKindPersistsItsPropertyListType() throws {
    let fixture = try FilePreferencesFixture(values: ["preferences.format": 1, "number": 0, "real": 1.0])
    defer { fixture.discard() }
    fixture.store.set(false, forKey: "number")
    fixture.store.set(1, forKey: "real")
    let values = try fixture.saved()
    #expect(CFGetTypeID(try #require(values["number"] as? NSNumber)) == CFBooleanGetTypeID())
    #expect(!["f", "d"].contains(String(cString: try #require(values["real"] as? NSNumber).objCType)))
  }

  @Test func everyReadSeesAtomicReplacementDeletionAndRecreation() throws {
    let fixture = try FilePreferencesFixture()
    defer { fixture.discard() }
    #expect(!fixture.store.bool(forKey: "shown"))
    try FilePreferencesFixture.write(["preferences.format": 1, "shown": true, "external": Data([8])], to: fixture.file)
    #expect(fixture.store.bool(forKey: "shown"))
    #expect(fixture.store.data(forKey: "external") == Data([8]))
    try FileManager.default.removeItem(at: fixture.file)
    #expect(fixture.store.object(forKey: "shown") == nil)
    #expect(fixture.store.refresh())
    try FilePreferencesFixture.write(["preferences.format": 1, "shown": false, "recreated": ["keep": true]], to: fixture.file)
    fixture.store.set("new", forKey: "text")
    #expect(fixture.store.string(forKey: "text") == "new")
    #expect(try fixture.saved()["recreated"] as? [String: Bool] == ["keep": true])
  }

  @Test func commitMergesOnlyEditedKeysIntoTheNewestValidatedFile() throws {
    let fixture = try FilePreferencesFixture(values: ["preferences.format": 1, "shown": false, "remove": "old"])
    defer { fixture.discard() }
    let notices = Mutex(0)
    let token = NotificationCenter.default.addObserver(forName: PreferenceStore.changed, object: fixture.store, queue: nil) { _ in
      notices.withLock { $0 += 1 }
    }
    defer { NotificationCenter.default.removeObserver(token) }
    try fixture.store.transaction(validate: { values in
      guard values["preferences.format"] as? Int == 1 else { throw Refusal.future }
    }) {
      fixture.store.set(true, forKey: "shown")
      fixture.store.removeObject(forKey: "remove")
      #expect(fixture.store.bool(forKey: "shown"), "the transaction reads its own edits")
      #expect(fixture.store.synchronize(), "synchronize stages rather than committing half a group")
      #expect(try fixture.saved()["shown"] as? Bool == false)
      try FilePreferencesFixture.write(["preferences.format": 1, "shown": false, "remove": "old", "external": ["data": Data([7])]],
        to: fixture.file)
    }
    #expect(notices.withLock { $0 } == 1)
    #expect(fixture.store.bool(forKey: "shown"))
    #expect(fixture.store.object(forKey: "remove") == nil)
    #expect(try fixture.saved()["external"] as? NSDictionary == ["data": Data([7])] as NSDictionary)
  }

  @Test func aNewerHeaderAtCommitRejectsEditsWithoutPublishing() throws {
    let fixture = try FilePreferencesFixture()
    defer { fixture.discard() }
    let notices = Mutex(0)
    let token = NotificationCenter.default.addObserver(forName: PreferenceStore.changed, object: fixture.store, queue: nil) { _ in
      notices.withLock { $0 += 1 }
    }
    defer { NotificationCenter.default.removeObserver(token) }
    var replaced = Data()
    #expect(throws: Refusal.self) {
      try fixture.store.transaction(validate: { values in
        guard values["preferences.format"] as? Int == 1 else { throw Refusal.future }
      }) {
        fixture.store.set(true, forKey: "shown")
        try FilePreferencesFixture.write(["preferences.format": 99, "shown": false, "future": "keep"], to: fixture.file)
        replaced = try Data(contentsOf: fixture.file)
      }
    }
    #expect(try Data(contentsOf: fixture.file) == replaced)
    #expect(!fixture.store.bool(forKey: "shown"))
    #expect(notices.withLock { $0 } == 0)
  }

  @Test func bodyAndSerializationFailureKeepTheEffectiveAndSavedValues() throws {
    let fixture = try FilePreferencesFixture()
    defer { fixture.discard() }
    let before = try Data(contentsOf: fixture.file)
    let notices = Mutex(0)
    let token = NotificationCenter.default.addObserver(forName: PreferenceStore.changed, object: fixture.store, queue: nil) { _ in
      notices.withLock { $0 += 1 }
    }
    defer { NotificationCenter.default.removeObserver(token) }
    #expect(throws: Refusal.self) {
      try fixture.store.transaction(validate: { _ in }) {
        fixture.store.set(true, forKey: "shown")
        throw Refusal.stopped
      }
    }
    #expect(!fixture.store.bool(forKey: "shown"))
    #expect(try Data(contentsOf: fixture.file) == before)
    #expect(throws: (any Error).self) {
      try fixture.store.transaction(validate: { _ in }) { fixture.store.set(NSObject(), forKey: "invalid") }
    }
    #expect(!fixture.store.synchronize())
    #expect(fixture.store.object(forKey: "invalid") == nil)
    #expect(try Data(contentsOf: fixture.file) == before)
    #expect(notices.withLock { $0 } == 0)
  }

  @Test func unwritableAndNonFileTargetsNeverReportSuccessfulPersistence() throws {
    let fixture = try FilePreferencesFixture()
    defer { fixture.discard() }
    let before = try Data(contentsOf: fixture.file)
    try FileManager.default.setAttributes([.posixPermissions: 0o500], ofItemAtPath: fixture.folder.path)
    fixture.store.set(true, forKey: "shown")
    #expect(!fixture.store.synchronize())
    #expect(!fixture.store.bool(forKey: "shown"))
    #expect(try Data(contentsOf: fixture.file) == before)
    #expect(throws: (any Error).self) {
      try fixture.store.transaction(validate: { _ in }) { fixture.store.set(true, forKey: "shown") }
    }
    try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: fixture.folder.path)
    try fixture.store.transaction(validate: { _ in }) {
      fixture.store.set(true, forKey: "shown")
      #expect(fixture.store.synchronize(), "an earlier failed mutation does not poison a new staged transaction")
    }
    #expect(fixture.store.synchronize())
    #expect(fixture.store.bool(forKey: "shown"))
    try FileManager.default.removeItem(at: fixture.file)
    try FileManager.default.createDirectory(at: fixture.file, withIntermediateDirectories: false)
    fixture.store.set(false, forKey: "shown")
    #expect(!fixture.store.synchronize())
    #expect(try fixture.file.resourceValues(forKeys: [.isDirectoryKey]).isDirectory == true)
  }

  @Test func concurrentStoresDoNotLoseIndependentKeys() throws {
    let fixture = try FilePreferencesFixture()
    defer { fixture.discard() }
    DispatchQueue.concurrentPerform(iterations: 24) { index in
      let store = PreferenceStore(file: fixture.file)
      store.set(index, forKey: "key.\(index)")
      #expect(store.synchronize())
    }
    let saved = try fixture.saved()
    for index in 0..<24 { #expect(saved["key.\(index)"] as? Int == index) }
    #expect(saved["preferences.format"] as? Int == 1)
    let lockFile = fixture.file.appendingPathExtension("lock")
    #expect(try FileManager.default.attributesOfItem(atPath: lockFile.path)[.posixPermissions] as? Int == 0o600)
    #expect(try FileManager.default.attributesOfItem(atPath: fixture.file.path)[.posixPermissions] as? Int == 0o600)
  }

  @Test func nativeURLRepresentationIsReadableInBothBackends() throws {
    let suite = "test.jevision.preference-url." + UUID().uuidString
    let native = try #require(UserDefaults(suiteName: suite))
    defer { native.removePersistentDomain(forName: suite); native.synchronize() }
    let fixture = try FilePreferencesFixture()
    defer { fixture.discard() }
    for url in [URL(filePath: NSHomeDirectory() + "/Library/Test file"), URL(string: "https://example.invalid/path?q=1#part")!] {
      native.set(url, forKey: "url")
      let representation = try #require(native.object(forKey: "url"))
      fixture.store.set(representation, forKey: "native")
      #expect(fixture.store.url(forKey: "native") == url)
      fixture.store.set(url, forKey: "profile")
      native.set(fixture.store.object(forKey: "profile"), forKey: "profile")
      #expect(native.url(forKey: "profile") == url)
      #expect(fixture.store.url(forKey: "profile") == url)
      #expect((representation is String) == (fixture.store.object(forKey: "profile") is String))
    }
    let adapter = native.preferenceStore
    #expect(adapter.nativeDefaults === native)
    #expect(adapter.identity == ObjectIdentifier(native))
    #expect(adapter.fileURL == nil)
  }

  @Test func argumentAndRegistrationValuesNeverEnterTheStoredSnapshot() throws {
    let fixture = try FilePreferencesFixture()
    defer { fixture.discard() }
    fixture.store.register(defaults: ["fallback": true, "preferences.format": 12])
    fixture.store.setVolatileDomain(["shown": true, "preferences.format": 99], forName: UserDefaults.argumentDomain)
    #expect(fixture.store.bool(forKey: "shown"))
    #expect(fixture.store.bool(forKey: "fallback"))
    #expect(fixture.store.integer(forKey: "preferences.format") == 99)
    #expect(fixture.store.persistentDomain(forName: fixture.domain)?["preferences.format"] as? Int == 1)
    #expect(fixture.store.persistentDomain(forName: fixture.domain)?["fallback"] == nil)
    #expect(try fixture.saved()["shown"] as? Bool == false)
  }
}
