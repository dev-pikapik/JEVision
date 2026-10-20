import CoreGraphics
import Foundation

/// The source of every event JEVision posts. Its events carry `marker`, so a person's input can
/// be told apart from the test's own. A private state also keeps keys the person holds down out
/// of the keys the test presses.
public enum SyntheticInput {
  /// "JEVISION" in ASCII.
  public static let marker: Int64 = 0x4A45_5649_5349_4F4E

  nonisolated(unsafe) static let source: CGEventSource? = {
    let source = CGEventSource(stateID: .privateState)
    source?.userData = marker
    // Synthetic events must not hold back the person's own mouse and keyboard.
    source?.localEventsSuppressionInterval = 0
    return source
  }()

  public static func isSynthetic(_ event: CGEvent) -> Bool {
    event.getIntegerValueField(.eventSourceUserData) == marker
  }
}

/// Two presses of Esc in a row, close enough in time, stop a run. Held-down Esc repeats and any
/// other key in between don't count.
public struct EscapeDetector: Sendable {
  public static let window: TimeInterval = 0.6
  private var lastEscape: TimeInterval?

  public init() {}

  /// Feeds one key press; returns `true` on the second Esc of a pair.
  public mutating func keyDown(isEscape: Bool, isRepeat: Bool, at time: TimeInterval) -> Bool {
    guard !isRepeat else { return false }
    guard isEscape else {
      lastEscape = nil
      return false
    }
    if let last = lastEscape, time - last <= Self.window {
      lastEscape = nil
      return true
    }
    lastEscape = time
    return false
  }
}

/// Watches the person's mouse and keyboard while a test drives the computer: notes any real
/// movement or key press (synthetic input doesn't count) and calls `onEmergencyStop` on Esc Esc,
/// whichever app is in front. What counts and what stops the run is up to `settings`, asked each
/// time. Needs Accessibility; runs its own thread.
public final class InputWatcher: @unchecked Sendable {
  private let lock = NSLock()
  private var touched = false
  private var quietUntil = Date.distantPast
  private var escape = EscapeDetector()
  private var onEmergencyStop: (@Sendable () -> Void)?
  private var tap: EventTap?
  private let settings: @Sendable () -> RunInputSettings

  public init(settings: @escaping @Sendable () -> RunInputSettings = { .app }) {
    self.settings = settings
  }

  /// Starts watching; `false` when the system doesn't allow it (Accessibility is off).
  public func start(onEmergencyStop: @escaping @Sendable () -> Void) -> Bool {
    stop()
    arm(onEmergencyStop: onEmergencyStop)
    let tap = EventTap(name: "JEVision input watcher") { [weak self] type, event in self?.handle(type, event) }
    let types: [CGEventType] = [
      .mouseMoved, .leftMouseDown, .rightMouseDown, .otherMouseDown, .leftMouseDragged,
      .rightMouseDragged, .scrollWheel, .keyDown,
    ]
    guard tap.start(types) else { return false }
    lock.withLock { self.tap = tap }
    return true
  }

  /// What `start` does before listening: the stop to call, with nothing seen yet.
  func arm(onEmergencyStop: @escaping @Sendable () -> Void) {
    lock.withLock {
      self.onEmergencyStop = onEmergencyStop
      touched = false
      escape = EscapeDetector()
    }
  }

  public func stop() {
    let tap = lock.withLock {
      defer {
        self.tap = nil
        onEmergencyStop = nil
      }
      return self.tap
    }
    tap?.stop()
  }

  /// Whether the person moved the mouse or typed since the last call, unless the settings say
  /// a run doesn't pause for that.
  public func takeUserInput() -> Bool {
    let pauses = settings().pausesOnMouse
    return lock.withLock {
      defer { touched = false }
      return touched && pauses
    }
  }

  /// Forgets input so far and ignores it for `seconds`: time to take the hand off the mouse
  /// after pressing Resume.
  public func ignoreInput(for seconds: TimeInterval) {
    lock.withLock {
      touched = false
      quietUntil = Date(timeIntervalSinceNow: seconds)
    }
  }

  private func handle(_ type: CGEventType, _ event: CGEvent) {
    guard !SyntheticInput.isSynthetic(event) else { return }
    note(
      key: type == .keyDown
        ? (
          isEscape: event.getIntegerValueField(.keyboardEventKeycode) == KeyStroke.escapeKeyCode,
          isRepeat: event.getIntegerValueField(.keyboardEventAutorepeat) != 0
        ) : nil,
      at: ProcessInfo.processInfo.systemUptime)
  }

  /// A person's event: any of them counts as a touch; a key press may also finish Esc Esc.
  func note(key: (isEscape: Bool, isRepeat: Bool)?, at time: TimeInterval) {
    // Asked only for key presses, which are few; a mouse move is just noted.
    let stops = key != nil && settings().escapeStops
    let stop: (@Sendable () -> Void)? = lock.withLock {
      if Date() >= quietUntil { touched = true }
      guard let key else { return nil }
      let second = escape.keyDown(isEscape: key.isEscape, isRepeat: key.isRepeat, at: time)
      return second && stops ? onEmergencyStop : nil
    }
    stop?()
  }
}

/// A listen-only tap on the session's mouse and keyboard, served on a thread of its own: it sees
/// input without holding any back. Needs Accessibility.
final class EventTap: @unchecked Sendable {
  private let name: String
  private let handle: @Sendable (CGEventType, CGEvent) -> Void
  private let lock = NSLock()
  private var tap: CFMachPort?
  private var runLoop: CFRunLoop?

  /// `handle` is called on the tap's thread and should return quickly.
  init(name: String, handle: @escaping @Sendable (CGEventType, CGEvent) -> Void) {
    self.name = name
    self.handle = handle
  }

  /// `false` when the system doesn't allow it (Accessibility is off).
  func start(_ types: [CGEventType]) -> Bool {
    let mask = types.reduce(CGEventMask(0)) { $0 | (1 << $1.rawValue) }
    guard
      let tap = CGEvent.tapCreate(
        tap: .cgSessionEventTap, place: .headInsertEventTap, options: .listenOnly,
        eventsOfInterest: mask, callback: eventTapCallback,
        userInfo: Unmanaged.passUnretained(self).toOpaque())
    else { return false }
    lock.withLock { self.tap = tap }
    let ready = DispatchSemaphore(value: 0)
    let thread = Thread { [self] in
      let loop = CFRunLoopGetCurrent()
      guard let tap = lock.withLock({ runLoop = loop; return self.tap }) else {
        ready.signal()
        return
      }
      let source = CFMachPortCreateRunLoopSource(nil, tap, 0)
      CFRunLoopAddSource(loop, source, .commonModes)
      CGEvent.tapEnable(tap: tap, enable: true)
      ready.signal()
      CFRunLoopRun()
    }
    thread.name = name
    thread.start()
    ready.wait()
    return true
  }

  func stop() {
    let (tap, loop) = lock.withLock {
      defer {
        self.tap = nil
        runLoop = nil
      }
      return (self.tap, runLoop)
    }
    if let tap {
      CGEvent.tapEnable(tap: tap, enable: false)
      CFMachPortInvalidate(tap)
    }
    if let loop { CFRunLoopStop(loop) }
  }

  fileprivate func receive(_ type: CGEventType, _ event: CGEvent) {
    if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
      if let tap = lock.withLock({ tap }) { CGEvent.tapEnable(tap: tap, enable: true) }
      return
    }
    handle(type, event)
  }
}

private func eventTapCallback(
  proxy: CGEventTapProxy, type: CGEventType, event: CGEvent, info: UnsafeMutableRawPointer?
) -> Unmanaged<CGEvent>? {
  if let info {
    Unmanaged<EventTap>.fromOpaque(info).takeUnretainedValue().receive(type, event)
  }
  return Unmanaged.passUnretained(event)
}
