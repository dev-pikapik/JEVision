import Foundation
import LocalAuthentication
import Observation

/// How long the app may stay in the background before it asks for Face ID again.
enum LockTimeout: Int, CaseIterable, Identifiable, Sendable {
  case immediately = 0
  case oneMinute = 60
  case fiveMinutes = 300
  case fifteenMinutes = 900

  var id: Int { rawValue }
  var interval: TimeInterval { TimeInterval(rawValue) }
}

/// When an unlocked app must lock again. Pure, so the rules are easy to check.
struct LockPolicy: Sendable {
  var timeout: LockTimeout
  /// However active the app is, an unlocked session ends after this long.
  var sessionLifetime: TimeInterval = 8 * 60 * 60

  func mustLock(unlockedAt: Date, backgroundedAt: Date?, now: Date) -> Bool {
    if now.timeIntervalSince(unlockedAt) >= sessionLifetime { return true }
    guard let backgroundedAt else { return false }
    return now.timeIntervalSince(backgroundedAt) >= timeout.interval
  }
}

/// Face ID or the passcode on launch, after time in the background, and when the session expires.
@MainActor @Observable
final class AppLock {
  enum State: Equatable {
    case locked
    case unlocking
    case unlocked(since: Date)
  }

  enum Problem: Equatable {
    /// The iPhone has no passcode, so there is nothing to protect the Macs with.
    case noPasscode
    case failed
  }

  /// Asks the owner to prove who they are and returns the context that did it.
  typealias Authenticate = @MainActor (_ reason: String) async throws -> LAContext

  private(set) var state: State = .locked
  private(set) var problem: Problem?
  private(set) var context: LAContext?
  var timeout: LockTimeout {
    didSet { defaults.set(timeout.rawValue, forKey: Self.timeoutKey) }
  }

  private var backgroundedAt: Date?
  private let authenticate: Authenticate
  private let defaults: UserDefaults
  private let now: @MainActor () -> Date
  /// Told about every new context, and `nil` on lock, so the device key follows the lock.
  var onChange: (@MainActor (LAContext?) -> Void)?

  private static let timeoutKey = "lockTimeout"

  init(
    defaults: UserDefaults = .standard,
    now: @escaping @MainActor () -> Date = { Date() },
    authenticate: @escaping Authenticate = AppLock.deviceOwner
  ) {
    self.defaults = defaults
    self.now = now
    self.authenticate = authenticate
    timeout = LockTimeout(rawValue: defaults.object(forKey: Self.timeoutKey) as? Int ?? -1) ?? .oneMinute
  }

  var isUnlocked: Bool {
    if case .unlocked = state { true } else { false }
  }

  func unlock(reason: String) async {
    guard state == .locked else { return }
    state = .unlocking
    problem = nil
    do {
      let context = try await authenticate(reason)
      self.context = context
      state = .unlocked(since: now())
      backgroundedAt = nil
      onChange?(context)
    } catch let error as LAError where error.code == .passcodeNotSet {
      problem = .noPasscode
      state = .locked
    } catch let error as LAError where [.userCancel, .appCancel, .systemCancel].contains(error.code) {
      state = .locked
    } catch {
      problem = .failed
      state = .locked
    }
  }

  func lock() {
    context?.invalidate()
    context = nil
    state = .locked
    backgroundedAt = nil
    onChange?(nil)
  }

  func didEnterBackground() {
    if isUnlocked { backgroundedAt = now() }
  }

  /// Call when the app comes back to the screen and on every check of the session.
  func checkExpiry() {
    guard case .unlocked(let since) = state else { return }
    if LockPolicy(timeout: timeout).mustLock(unlockedAt: since, backgroundedAt: backgroundedAt, now: now()) {
      lock()
    } else {
      backgroundedAt = nil
    }
  }

  static func deviceOwner(reason: String) async throws -> LAContext {
    let context = LAContext()
    try await context.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason)
    return context
  }
}
