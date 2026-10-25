import AppKit
import ApplicationServices
import JEVisionCore

/// Records what a person does in one app. Watches clicks and keys without holding any back,
/// keeps a fresh picture of the app's front window so each click has a picture from just before
/// it, asks Accessibility what was clicked, and feeds it all to a `Recording`. Input in other apps
/// and in JEVision's own panels doesn't count. Needs Accessibility and Screen Recording.
public final class Recorder: @unchecked Sendable {
  public enum Notice: Sendable, Equatable {
    /// A click outside the app's window, in its menus say: a test can't repeat it.
    case outsideWindow
    /// A drag let go outside the app's window: a test can't repeat it.
    case dragOutsideWindow
    /// No picture of the app's window to record the click with.
    case noWindow
    /// The area to check isn't over the app's window.
    case checkOutsideWindow
  }

  public struct Update: Sendable {
    public let steps: [RecordedStep]
    public let notice: Notice?
  }

  /// Pause between two pictures of the window.
  static let frameInterval: useconds_t = 100_000
  /// Time an app takes to move the keyboard after a click or Tab.
  static let focusDelay: TimeInterval = 0.25

  private let app: String
  /// The app's process when it has the keyboard, `nil` when another app is in front.
  private let front: @Sendable () -> pid_t?
  /// The control that has the keyboard in a process.
  private let focusedElement: @Sendable (pid_t) -> ElementInfo?
  private let queue = DispatchQueue(label: "JEVision recorder", qos: .userInitiated)
  /// Touched only on `queue`.
  private var recording: Recording
  /// The last answer to what has the keyboard, and when it was asked. Touched only on `queue`.
  private var lastFocus: (pid: pid_t, asked: TimeInterval, element: ElementInfo?)?

  private let lock = NSLock()
  private var tap: EventTap?
  private var running = false
  private var ignoring = false
  /// The latest pictures of the window, newest last, with the time each was ready.
  private var frames: [(frame: WindowFrame, ready: TimeInterval)] = []
  private var pids: Set<pid_t> = []
  private var onUpdate: (@Sendable (Update) -> Void)?
  private var onStop: (@Sendable () -> Void)?

  /// Records in the app with bundle id `app`; see `Recording.init` for the rest.
  public convenience init(app: String, appName: String, opensApp: Bool) {
    self.init(
      app: app, appName: appName, opensApp: opensApp,
      front: { MacHands.runningApps(app).first(where: \.isActive)?.processIdentifier },
      focusedElement: { Accessibility.focusedElement(pid: $0) })
  }

  init(
    app: String, appName: String, opensApp: Bool, front: @escaping @Sendable () -> pid_t?,
    focusedElement: @escaping @Sendable (pid_t) -> ElementInfo?
  ) {
    self.app = app
    self.front = front
    self.focusedElement = focusedElement
    recording = Recording(appName: appName, opensApp: opensApp)
  }

  /// Starts recording; `false` when the system doesn't allow watching input (Accessibility is
  /// off). `onUpdate` and `onStop` (Esc Esc) are called on a background thread.
  public func start(onUpdate: @escaping @Sendable (Update) -> Void, onStop: @escaping @Sendable () -> Void) -> Bool {
    let tap = EventTap(name: "JEVision recorder") { [weak self] type, event in self?.receive(type, event) }
    lock.withLock {
      self.onUpdate = onUpdate
      self.onStop = onStop
      pids = Set(MacHands.runningApps(app).map(\.processIdentifier))
      running = true
    }
    let thread = Thread { [weak self] in
      while let self, self.lock.withLock({ self.running }) {
        self.refreshFrame()
        usleep(Self.frameInterval)
      }
    }
    thread.name = "JEVision recorder pictures"
    thread.start()
    let types: [CGEventType] = [
      .leftMouseDown, .leftMouseUp, .rightMouseDown, .rightMouseUp, .otherMouseDown, .otherMouseUp, .scrollWheel, .keyDown,
    ]
    guard tap.start(types) else {
      lock.withLock { running = false }
      return false
    }
    lock.withLock { self.tap = tap }
    return true
  }

  /// The app's front window as last seen (global points, top-left origin).
  public var windowBounds: CGRect? {
    lock.withLock { frames.last?.frame.bounds }
  }

  /// Ignores input while paused or while a JEVision panel takes it; Esc Esc still stops.
  public func setIgnoringInput(_ on: Bool) {
    lock.withLock { ignoring = on }
  }

  public func undoLastStep() {
    queue.async { [self] in
      recording.undoLastStep()
      publish(nil)
    }
  }

  /// Adds a wait of a fixed time after the last step.
  public func addWait() {
    queue.async { [self] in
      recording.wait()
      publish(nil)
    }
  }

  /// Adds a check that `area` (global points, top-left origin) looks as it does now.
  public func addCheck(area: CGRect) {
    queue.async { [self] in
      guard let shot = try? MacHands.captureFrontWindow(app), let frame = Self.frame(shot) else {
        return publish(.noWindow)
      }
      let label = Accessibility.element(at: CGPoint(x: area.midX, y: area.midY), pids: currentPIDs())?.label
      publish(recording.check(area: area, frame: frame, label: label) ? nil : .checkOutsideWindow)
    }
  }

  /// Stops watching and hands over the recording once every event so far is in it.
  public func finish(_ done: @escaping @Sendable (Recording) -> Void) {
    let tap = lock.withLock {
      defer {
        self.tap = nil
        running = false
        onUpdate = nil
        onStop = nil
      }
      return self.tap
    }
    tap?.stop()
    queue.async { [self] in done(recording) }
  }

  // MARK: Input

  func receive(_ type: CGEventType, _ event: CGEvent) {
    guard !SyntheticInput.isSynthetic(event) else { return }
    let now = ProcessInfo.processInfo.systemUptime
    let state: (ignoring: Bool, frame: WindowFrame?) = lock.withLock {
      let latest: WindowFrame? = frames.last(where: { $0.ready < now })?.frame
      return (self.ignoring, latest)
    }
    let ignoring = state.ignoring, frame = state.frame
    // Of the other buttons only the middle one has a block to repeat it; the rest go back and forward.
    if type == .otherMouseDown || type == .otherMouseUp, event.getIntegerValueField(.mouseEventButtonNumber) != 2 { return }
    if type == .keyDown {
      // A tool or an input method may put a whole string into one press: read all of it.
      var units: [UniChar] = []
      var length = 0
      repeat {
        units = [UniChar](repeating: 0, count: max(16, units.count * 2))
        event.keyboardGetUnicodeString(maxStringLength: units.count, actualStringLength: &length, unicodeString: &units)
      } while length == units.count
      let flags = event.flags
      var modifiers: KeyPress.Modifiers = []
      if flags.contains(.maskCommand) { modifiers.insert(.command) }
      if flags.contains(.maskShift) { modifiers.insert(.shift) }
      if flags.contains(.maskAlternate) { modifiers.insert(.option) }
      if flags.contains(.maskControl) { modifiers.insert(.control) }
      let press = KeyPress(
        code: CGKeyCode(event.getIntegerValueField(.keyboardEventKeycode)),
        characters: String(utf16CodeUnits: units, count: length), modifiers: modifiers,
        isRepeat: event.getIntegerValueField(.keyboardEventAutorepeat) != 0, time: now)
      // Whose press it is, decided now: keys typed faster than the queue handles them would
      // otherwise count for whichever app is in front by the time their turn comes.
      let front = ignoring ? nil : front()
      queue.async { [self] in key(press, front: front, frame: frame) }
    } else if type == .leftMouseUp || type == .rightMouseUp || type == .otherMouseUp {
      let location = event.location
      let button = Self.button(of: type)
      queue.async { [self] in release(at: location, button: button) }
    } else if type == .scrollWheel {
      let location = event.location
      // A trackpad scrolls by pixels, a mouse wheel by lines.
      let pixels = event.getIntegerValueField(.scrollWheelEventIsContinuous) != 0
      let vertical = Int(event.getIntegerValueField(pixels ? .scrollWheelEventPointDeltaAxis1 : .scrollWheelEventDeltaAxis1))
      let horizontal = Int(event.getIntegerValueField(pixels ? .scrollWheelEventPointDeltaAxis2 : .scrollWheelEventDeltaAxis2))
      let owner = Self.windowOwner(at: location)
      queue.async { [self] in
        let inApp = !ignoring && owner.map(currentPIDs().contains) == true
        switch recording.scroll(at: location, vertical: vertical, horizontal: horizontal, pixels: pixels, inApp: inApp, frame: frame) {
        case .recorded: publish(nil)
        case .outsideWindow where frame == nil: publish(.noWindow)
        case .outsideWindow, .ignored: break
        }
      }
    } else {
      let location = event.location
      let button = Self.button(of: type)
      let count = Int(event.getIntegerValueField(.mouseEventClickState))
      let modifiers = Modifiers(flags: event.flags)
      // Whose window is under the pointer now, before the click changes anything.
      let owner = Self.windowOwner(at: location)
      queue.async { [self] in
        click(at: location, button: button, modifiers: modifiers, count: count, owner: owner, ignoring: ignoring, frame: frame)
      }
    }
  }

  /// The button a press or a release of `type` is about.
  static func button(of type: CGEventType) -> MouseButton {
    switch type {
    case .rightMouseDown, .rightMouseUp: .right
    case .otherMouseDown, .otherMouseUp: .middle
    default: .left
    }
  }

  private func click(
    at point: CGPoint, button: MouseButton, modifiers: Modifiers, count: Int, owner: pid_t?, ignoring: Bool, frame: WindowFrame?
  ) {
    let inApp = !ignoring && owner.map(currentPIDs().contains) == true
    let element = inApp ? Accessibility.element(at: point, pids: currentPIDs()) : nil
    let result = recording.click(
      at: point, button: button, modifiers: modifiers, count: count, inApp: inApp, frame: frame, element: element)
    switch result {
    case .recorded:
      publish(nil)
      checkFocusSoon()
    case .outsideWindow: publish(frame == nil ? .noWindow : .outsideWindow)
    case .ignored: break
    }
  }

  private func release(at point: CGPoint, button: MouseButton) {
    let element = Accessibility.element(at: point, pids: currentPIDs())
    switch recording.release(at: point, button: button, element: element) {
    case .recorded: publish(nil)
    case .outsideWindow: publish(.dragOutsideWindow)
    case .ignored: break
    }
  }

  /// `front`: the app's process that had the keyboard at the press, `nil` for another app.
  private func key(_ press: KeyPress, front: pid_t?, frame: WindowFrame?) {
    let inApp = front != nil
    let focused = front.flatMap { self.focus(of: $0, at: press.time) }
    switch recording.key(press, inApp: inApp, frame: frame, focus: focused) {
    case .stop:
      lock.withLock { onStop }?()
    case .recorded:
      publish(nil)
      if [48, 36, 76].contains(press.code) { checkFocusSoon() }
    case .ignored:
      if inApp { checkFocusSoon() }
    }
  }

  /// What has the keyboard of `pid` for a press at `time`. Any answer asked after the press is as
  /// good as a fresh one, and closer to the press: keys typed faster than Accessibility answers
  /// share one answer instead of each waiting for its own, and a password field that has the
  /// keyboard at the press is still seen.
  private func focus(of pid: pid_t, at time: TimeInterval) -> ElementInfo? {
    if let last = lastFocus, last.pid == pid, last.asked >= time { return last.element }
    let asked = ProcessInfo.processInfo.systemUptime
    let element = focusedElement(pid)
    lastFocus = (pid, asked, element)
    return element
  }

  /// A password field that takes the keyboard becomes a secret even though its keys stay hidden.
  private func checkFocusSoon() {
    queue.asyncAfter(deadline: .now() + Self.focusDelay) { [self] in
      guard let front = front(), let focus = focusedElement(front), focus.isSecure
      else { return }
      let count = recording.steps.count
      recording.focusMoved(to: focus, frame: lock.withLock { frames.last?.frame })
      if recording.steps.count != count { publish(nil) }
    }
  }

  private func publish(_ notice: Notice?) {
    let update = Update(steps: recording.steps, notice: notice)
    lock.withLock { onUpdate }?(update)
  }

  // MARK: Pictures

  private func refreshFrame() {
    let pids = Set(MacHands.runningApps(app).map(\.processIdentifier))
    let frame = (try? MacHands.captureFrontWindow(app)).flatMap(Self.frame)
    let ready = ProcessInfo.processInfo.systemUptime
    lock.withLock {
      self.pids = pids
      guard let frame else { return frames = [] }
      frames = Array((frames + [(frame, ready)]).suffix(3))
    }
  }

  private static func frame(
    _ shot: (image: CGImage, scale: Double, window: (id: CGWindowID, bounds: CGRect, title: String?))
  ) -> WindowFrame? {
    MacHands.bitmap(shot.image).map {
      WindowFrame(window: shot.window.id, bounds: shot.window.bounds, scale: shot.scale, title: shot.window.title, bitmap: $0)
    }
  }

  private func currentPIDs() -> Set<pid_t> {
    lock.withLock { pids }
  }

  /// The app whose window is frontmost at `point` (global, top-left origin), menus included.
  static func windowOwner(at point: CGPoint) -> pid_t? {
    guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]]
    else { return nil }
    for info in list {
      guard let dict = info[kCGWindowBounds as String] as? NSDictionary,
        let bounds = CGRect(dictionaryRepresentation: dict), bounds.contains(point),
        (info[kCGWindowAlpha as String] as? Double ?? 1) > 0
      else { continue }
      return info[kCGWindowOwnerPID as String] as? pid_t
    }
    return nil
  }
}

/// What Accessibility tells about the controls of other apps.
enum Accessibility {
  /// Long enough for a busy app, short enough not to hold up the recording.
  static let timeout: Float = 0.3

  static func element(at point: CGPoint, pids: Set<pid_t>) -> ElementInfo? {
    for pid in pids {
      let app = AXUIElementCreateApplication(pid)
      AXUIElementSetMessagingTimeout(app, timeout)
      var element: AXUIElement?
      if AXUIElementCopyElementAtPosition(app, Float(point.x), Float(point.y), &element) == .success, let element {
        return info(element)
      }
    }
    return nil
  }

  /// Password fields in the window at `bounds` (global points, top-left origin) of one of
  /// `pids`, in fractions of the window. Empty without the Accessibility permission. A window
  /// too large to search within a quarter of a second gives the fields found by then.
  static func secureFields(pids: Set<pid_t>, window bounds: CGRect) -> [Rect] {
    guard AXIsProcessTrusted(), bounds.width > 0, bounds.height > 0 else { return [] }
    let deadline = Date.now.addingTimeInterval(0.25)
    for pid in pids {
      let app = AXUIElementCreateApplication(pid)
      AXUIElementSetMessagingTimeout(app, timeout)
      var windows: CFTypeRef?
      guard AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &windows) == .success,
        let window = (windows as? [AXUIElement])?.first(where: {
          frame(of: $0).map { abs($0.minX - bounds.minX) < 2 && abs($0.minY - bounds.minY) < 2 } == true
        })
      else { continue }
      var fields: [Rect] = []
      var queue = [window]
      var next = 0
      while next < queue.count, next < 5000, Date.now < deadline {
        let element = queue[next]
        next += 1
        // One round trip to the app for both attributes.
        var values: CFArray?
        guard
          AXUIElementCopyMultipleAttributeValues(
            element, [kAXSubroleAttribute, kAXChildrenAttribute] as CFArray, AXCopyMultipleAttributeOptions(), &values)
            == .success,
          let values = values as? [AnyObject], values.count == 2
        else { continue }
        if values[0] as? String == kAXSecureTextFieldSubrole {
          if let f = frame(of: element) {
            fields.append(
              Rect(
                x: (f.minX - bounds.minX) / bounds.width, y: (f.minY - bounds.minY) / bounds.height,
                width: f.width / bounds.width, height: f.height / bounds.height))
          }
          continue
        }
        queue += values[1] as? [AXUIElement] ?? []
      }
      return fields
    }
    return []
  }

  static func frame(of element: AXUIElement) -> CGRect? {
    func value<T: BitwiseCopyable>(_ attribute: String, _ type: AXValueType, _ empty: T) -> T? {
      var raw: CFTypeRef?
      guard AXUIElementCopyAttributeValue(element, attribute as CFString, &raw) == .success, let raw,
        CFGetTypeID(raw) == AXValueGetTypeID()
      else { return nil }
      var out = empty
      return AXValueGetValue(raw as! AXValue, type, &out) ? out : nil
    }
    guard let origin = value(kAXPositionAttribute, .cgPoint, CGPoint.zero),
      let size = value(kAXSizeAttribute, .cgSize, CGSize.zero)
    else { return nil }
    return CGRect(origin: origin, size: size)
  }

  static func focusedElement(pid: pid_t) -> ElementInfo? {
    let app = AXUIElementCreateApplication(pid)
    AXUIElementSetMessagingTimeout(app, timeout)
    var focused: CFTypeRef?
    guard AXUIElementCopyAttributeValue(app, kAXFocusedUIElementAttribute as CFString, &focused) == .success,
      let focused, CFGetTypeID(focused) == AXUIElementGetTypeID()
    else { return nil }
    return info(focused as! AXUIElement)
  }

  private static func info(_ element: AXUIElement) -> ElementInfo {
    func string(_ attribute: String, of e: AXUIElement = element) -> String? {
      var value: CFTypeRef?
      guard AXUIElementCopyAttributeValue(e, attribute as CFString, &value) == .success,
        let text = value as? String, !text.trimmingCharacters(in: .whitespaces).isEmpty
      else { return nil }
      return text
    }
    let role = string(kAXRoleAttribute)
    var titleElement: CFTypeRef?
    let titled =
      AXUIElementCopyAttributeValue(element, kAXTitleUIElementAttribute as CFString, &titleElement) == .success
      && titleElement.map { CFGetTypeID($0) == AXUIElementGetTypeID() } == true
    let label =
      string(kAXTitleAttribute) ?? string(kAXDescriptionAttribute) ?? string(kAXPlaceholderValueAttribute)
      ?? (titled ? string(kAXValueAttribute, of: titleElement as! AXUIElement) : nil)
      ?? (role == "AXStaticText" ? string(kAXValueAttribute) : nil)
    return ElementInfo(role: role, subrole: string(kAXSubroleAttribute), label: label, frame: frame(of: element))
  }
}
