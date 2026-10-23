import Foundation
import Observation

/// Paired Macs, their connections and the app lock. Screens reach Macs only through here.
@MainActor @Observable
final class AppModel {
  enum Connection {
    case connecting
    case online(any MacSession)
    case unreachable(RemoteError)
  }

  enum StorageProblem {
    /// The paired Macs couldn't be saved; the pairing works until the app quits.
    case failed
    /// The list was saved by a newer JEVision Remote, which this version never writes over.
    case newerList
    /// The saved list couldn't be opened, so nothing is saved until it opens: its Macs would be lost.
    case notOpened
  }

  /// What to do when the saved list couldn't be opened.
  static var notOpenedMessage: String {
    String(localized: "The list of paired Macs couldn’t be opened. Unlock the iPhone and open the app again.")
  }

  private(set) var hosts: [HostRecord]
  private(set) var connections: [String: Connection] = [:]
  /// Why the paired Macs couldn't be saved, the last time it was tried.
  private(set) var storageProblem: StorageProblem?
  /// The saved list couldn't be read and the app started without it; set until the notice is seen.
  var listWasUnreadable: Bool
  /// The saved list couldn't be opened; set until the notice is seen or the list opens.
  var listDidNotOpen: Bool
  let lock: AppLock
  /// The camera is open to scan a Mac's pairing code.
  var isScanning = false
  /// A scanned or opened code waiting for the owner to confirm.
  var pairingLink: PairingLink?

  private let client: any RemoteClient
  private let store: HostStore
  private var pending: [String: Task<any MacSession, any Error>] = [:]

  init(client: any RemoteClient, store: HostStore = .standard, lock: AppLock) {
    self.client = client
    self.store = store
    self.lock = lock
    let saved = store.load()
    hosts = saved.hosts
    listWasUnreadable = saved.problem == .unreadable
    listDidNotOpen = saved.problem == .notOpened
    if listDidNotOpen { storageProblem = .notOpened }
  }

  /// Loads the saved list again if it couldn't be opened, as before the iPhone's first unlock.
  /// Macs paired in the meantime stay and are saved with it.
  func reopenList() {
    guard storageProblem == .notOpened else { return }
    let saved = store.load()
    guard saved.problem != .notOpened else { return }
    let paired = hosts.filter { host in !saved.hosts.contains { $0.id == host.id } }
    hosts = saved.hosts + paired
    listWasUnreadable = saved.problem == .unreadable
    listDidNotOpen = false
    storageProblem = nil
    if !paired.isEmpty { save() }
  }

  /// Nothing to protect until a Mac is paired, so a fresh install opens straight to pairing.
  var needsUnlock: Bool { !hosts.isEmpty && !lock.isUnlocked }

  /// Pairs after the owner confirms with Face ID or the passcode; `nil` if they didn't.
  func pair(_ link: PairingLink, deviceName: String, now: Date = Date()) async throws(RemoteError) -> HostRecord? {
    if link.isExpired(at: now) { throw .codeExpired }
    if !lock.isUnlocked {
      await lock.unlock(reason: String(localized: "Confirm it’s you to pair this iPhone with \(link.macName)."))
      guard lock.isUnlocked else { return nil }
    }
    let session = try await client.pair(offer: link.text, deviceName: deviceName)
    let record = session.record
    if case .online(let old) = connections[record.id], old !== session { await old.close() }
    connections[record.id] = .online(session)
    remember(record)
    return record
  }

  /// The signed-in connection to `host`, connecting first if needed.
  func session(for host: HostRecord) async throws(RemoteError) -> any MacSession {
    lock.checkExpiry()
    guard lock.isUnlocked else { throw .other }
    if case .online(let session) = connections[host.id] { return session }
    let task: Task<any MacSession, any Error>
    if let running = pending[host.id] {
      task = running
    } else {
      connections[host.id] = .connecting
      let client = client
      task = Task { try await client.connect(host) }
      pending[host.id] = task
    }
    do {
      let session = try await task.value
      if pending[host.id] == task {
        pending[host.id] = nil
        connections[host.id] = .online(session)
        remember(session.record)
      }
      return session
    } catch {
      let error = error as? RemoteError ?? .other
      if pending[host.id] == task {
        pending[host.id] = nil
        connections[host.id] = .unreachable(error)
      }
      throw error
    }
  }

  /// Runs a request on `host`, connecting first if needed.
  func fetch<T: Sendable>(
    _ host: HostRecord, _ request: (any MacSession) async throws(RemoteError) -> T
  ) async -> Loadable<T> {
    do {
      let session = try await session(for: host)
      return .loaded(try await request(session))
    } catch {
      connectionFailed(host, error)
      return .failed(error)
    }
  }

  /// What this iPhone may do on `host`, once connected.
  func permission(on host: HostRecord) -> Permission? {
    if case .online(let session) = connections[host.id] { session.permission } else { nil }
  }

  /// Connects to every paired Mac, so the list can say which are online.
  func connectAll() async {
    await withTaskGroup { group in
      for host in hosts {
        group.addTask { _ = try? await self.session(for: host) }
      }
    }
  }

  /// A request to `host` failed: the next one connects again, and the list shows why.
  func connectionFailed(_ host: HostRecord, _ error: RemoteError) {
    guard [.macOffline, .revoked].contains(error), case .online(let session) = connections[host.id] else { return }
    connections[host.id] = .unreachable(error)
    Task { await session.close() }
  }

  func forget(_ host: HostRecord) async {
    pending.removeValue(forKey: host.id)?.cancel()
    if case .online(let session) = connections.removeValue(forKey: host.id) { await session.close() }
    hosts.removeAll { $0.id == host.id }
    save()
    if hosts.isEmpty { lock.lock() }
  }

  /// Ends every connection; called when the app locks.
  func disconnectAll() {
    for task in pending.values { task.cancel() }
    pending = [:]
    let sessions = connections.values.compactMap { if case .online(let s) = $0 { s } else { nil } }
    connections = [:]
    Task { for session in sessions { await session.close() } }
  }

  private func remember(_ record: HostRecord) {
    if let index = hosts.firstIndex(where: { $0.id == record.id }) {
      guard hosts[index] != record else { return }
      hosts[index] = record
    } else {
      hosts.append(record)
    }
    save()
  }

  private func save() {
    // Until the list opens, writing would drop the Macs in it.
    guard storageProblem != .notOpened else { return }
    do {
      try store.save(hosts)
      storageProblem = nil
    } catch is HostStore.NewerList {
      storageProblem = .newerList
    } catch is HostStore.NotOpened {
      storageProblem = .notOpened
    } catch {
      storageProblem = .failed
    }
  }
}
