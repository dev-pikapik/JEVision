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
/// whichever app is in front. Needs Accessibility; runs its own thread.
public final class InputWatcher: @unchecked Sendable {
  private let lock = NSLock()
  private var touched = false
  private var quietUntil = Date.distantPast
  private var escape = EscapeDetector()
  private var onEmergencyStop: (@Sendable () -> Void)?
  private var tap: CFMachPort?
  private var runLoop: CFRunLoop?

  public init() {}

  /// Starts watching; `false` when the system doesn't allow it (Accessibility is off).
  public func start(onEmergencyStop: @escaping @Sendable () -> Void) -> Bool {
    stop()
    let types: [CGEventType] = [
      .mouseMoved, .leftMouseDown, .rightMouseDown, .otherMouseDown, .leftMouseDragged,
      .rightMouseDragged, .scrollWheel, .keyDown,
    ]
    let mask = types.reduce(CGEventMask(0)) { $0 | (1 << $1.rawValue) }
    guard
      let tap = CGEvent.tapCreate(
        tap: .cgSessionEventTap, place: .headInsertEventTap, options: .listenOnly,
        eventsOfInterest: mask, callback: inputWatcherCallback,
        userInfo: Unmanaged.passUnretained(self).toOpaque())
    else { return false }
    lock.withLock {
      self.tap = tap
      self.onEmergencyStop = onEmergencyStop
      touched = false
      escape = EscapeDetector()
    }
    let ready = DispatchSemaphore(value: 0)
    let thread = Thread { [self] in
      let loop = CFRunLoopGetCurrent()
      guard let tap = lock.withLock({ () -> CFMachPort? in
        runLoop = loop
        return self.tap
      }) else {
        ready.signal()
        return
      }
      let source = CFMachPortCreateRunLoopSource(nil, tap, 0)
      CFRunLoopAddSource(loop, source, .commonModes)
      CGEvent.tapEnable(tap: tap, enable: true)
      ready.signal()
      CFRunLoopRun()
    }
    thread.name = "JEVision input watcher"
    thread.start()
    ready.wait()
    return true
  }

  public func stop() {
    let (tap, loop) = lock.withLock {
      defer {
        self.tap = nil
        runLoop = nil
        onEmergencyStop = nil
      }
      return (self.tap, runLoop)
    }
    if let tap {
      CGEvent.tapEnable(tap: tap, enable: false)
      CFMachPortInvalidate(tap)
    }
    if let loop { CFRunLoopStop(loop) }
  }

  /// Whether the person moved the mouse or typed since the last call.
  public func takeUserInput() -> Bool {
    lock.withLock {
      defer { touched = false }
      return touched
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

  fileprivate func handle(_ type: CGEventType, _ event: CGEvent) {
    if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
      if let tap = lock.withLock({ tap }) { CGEvent.tapEnable(tap: tap, enable: true) }
      return
    }
    guard !SyntheticInput.isSynthetic(event) else { return }
    let stop: (@Sendable () -> Void)? = lock.withLock {
      if Date() >= quietUntil { touched = true }
      guard type == .keyDown else { return nil }
      let isEscape = event.getIntegerValueField(.keyboardEventKeycode) == KeyStroke.escapeKeyCode
      let isRepeat = event.getIntegerValueField(.keyboardEventAutorepeat) != 0
      let now = ProcessInfo.processInfo.systemUptime
      return escape.keyDown(isEscape: isEscape, isRepeat: isRepeat, at: now) ? onEmergencyStop : nil
    }
    stop?()
  }
}

private func inputWatcherCallback(
  proxy: CGEventTapProxy, type: CGEventType, event: CGEvent, info: UnsafeMutableRawPointer?
) -> Unmanaged<CGEvent>? {
  if let info {
    Unmanaged<InputWatcher>.fromOpaque(info).takeUnretainedValue().handle(type, event)
  }
  return Unmanaged.passUnretained(event)
}
