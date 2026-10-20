import AppKit
import ApplicationServices
import IOKit.pwr_mgt
import JEVisionCore
import ScreenCaptureKit
import UniformTypeIdentifiers

/// The macOS side of a run: launches apps, looks at their front window, clicks and types.
/// Called from the run's thread. Screenshots need Screen Recording; clicks and keys need
/// Accessibility.
public final class MacHands: Hands, @unchecked Sendable {
  /// How long an app may take to open a window.
  static let launchTimeout: TimeInterval = 20
  /// Pause between typed characters, so apps keep up.
  static let keyInterval: useconds_t = 8_000
  /// Pixels a single scroll event moves at most.
  static let pixelsPerScroll: Int32 = 40

  private let input: InputWatcher
  private let isCancelled: @Sendable () -> Bool
  private let clicking: @Sendable (CGRect?) -> Void
  /// Keeps the display on while a run has hands: a sleeping display gives no picture to look at.
  private var displayOn: IOPMAssertionID = 0
  /// The run's video while it records.
  private var video: RunVideo?
  /// Who opened each file or link (its path or address): the app, and whether opening it
  /// started that app.
  private var openers: [String: (pid: pid_t, launched: Bool)] = [:]

  /// `input` reports the person's own mouse and keyboard; `isCancelled` tells long actions to
  /// stop early. `clicking` hears the frame of the window about to be clicked (global, top-left
  /// origin) before the click is posted and `nil` after it, so floating panels can step aside
  /// and let the click through; it is called on the run's thread and must return only once done.
  public init(
    input: InputWatcher, isCancelled: @escaping @Sendable () -> Bool,
    clicking: @escaping @Sendable (CGRect?) -> Void = { _ in }
  ) {
    self.input = input
    self.isCancelled = isCancelled
    self.clicking = clicking
    IOPMAssertionCreateWithName(
      kIOPMAssertionTypePreventUserIdleDisplaySleep as CFString, IOPMAssertionLevel(kIOPMAssertionLevelOn),
      "JEVision is running a test" as CFString, &displayOn)
  }

  deinit {
    IOPMAssertionRelease(displayOn)
  }

  // MARK: Apps

  public func openApp(app: String) throws(HandsError) {
    let url = try appURL(app)
    let running = try launch(url)
    waitForWindow(running.map { [$0] } ?? Self.runningApps(app))
  }

  public func openItems(items: [Openable], app: String?, newInstance: Bool) throws(HandsError) {
    var groups: [URL: [URL]] = [:]
    for item in items {
      let (url, name): (URL?, String) =
        switch item {
        case .file(let path): (URL(fileURLWithPath: path), path)
        case .url(let address): (URL(string: address), address)
        }
      guard let url else { throw .CannotOpen(item: name) }
      let opener: URL
      if let app {
        opener = try appURL(app)
      } else if let found = NSWorkspace.shared.urlForApplication(toOpen: url) {
        opener = found
      } else {
        throw .CannotOpen(item: name)
      }
      groups[opener, default: []].append(url)
    }
    for (opener, urls) in groups {
      let id = Bundle(url: opener)?.bundleIdentifier
      let wasRunning = !newInstance && id.map { !Self.runningApps($0).isEmpty } == true
      let config = NSWorkspace.OpenConfiguration()
      config.activates = true
      config.createsNewApplicationInstance = newInstance
      let result = Self.wait { done in
        NSWorkspace.shared.open(urls, withApplicationAt: opener, configuration: config) { app, error in
          done((app?.processIdentifier, error?.localizedDescription))
        }
      }
      guard result.1 == nil else { throw .CannotOpen(item: urls[0].isFileURL ? urls[0].path : urls[0].absoluteString) }
      if let pid = result.0 {
        for url in urls { openers[url.isFileURL ? url.path : url.absoluteString] = (pid, !wasRunning) }
      }
      waitForWindow(result.0.flatMap { NSRunningApplication(processIdentifier: $0) }.map { [$0] } ?? [])
    }
  }

  /// Quits the apps that opening `items` started; in an app that was already running, closes
  /// only the windows showing those files. Links stay open there, so the person's own browser
  /// tabs are never touched.
  public func closeItems(items: [Openable], app: String?) throws(HandsError) {
    var started: Set<pid_t> = []
    for item in items {
      let key: String
      switch item {
      case .file(let path): key = path
      case .url(let address): key = address
      }
      guard let opener = openers.removeValue(forKey: key) else { continue }
      if opener.launched {
        started.insert(opener.pid)
      } else if case .file(let path) = item {
        Self.closeWindows(of: opener.pid, showing: URL(fileURLWithPath: path))
      }
    }
    let running = started.compactMap { NSRunningApplication(processIdentifier: $0) }
    running.forEach { $0.terminate() }
    _ = poll(timeout: 10) { running.allSatisfy(\.isTerminated) }
  }

  /// Presses the close button of every window of `pid` whose document is `file`.
  private static func closeWindows(of pid: pid_t, showing file: URL) {
    let app = AXUIElementCreateApplication(pid)
    var windows: CFTypeRef?
    guard AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &windows) == .success,
      let windows = windows as? [AXUIElement]
    else { return }
    for window in windows {
      var document: CFTypeRef?
      var button: CFTypeRef?
      guard AXUIElementCopyAttributeValue(window, kAXDocumentAttribute as CFString, &document) == .success,
        let address = document as? String, let url = URL(string: address),
        url.standardizedFileURL.path == file.standardizedFileURL.path,
        AXUIElementCopyAttributeValue(window, kAXCloseButtonAttribute as CFString, &button) == .success,
        let button, CFGetTypeID(button) == AXUIElementGetTypeID()
      else { continue }
      AXUIElementPerformAction(button as! AXUIElement, kAXPressAction as CFString)
    }
  }

  public func activate(app: String) throws(HandsError) {
    let running = Self.runningApps(app)
    guard let first = running.first else { return try openApp(app: app) }
    if running.contains(where: \.isActive) { return }
    _ = first.activate()
    if poll(timeout: 1, { first.isActive }) { return }
    // Without the user's click macOS may refuse; Launch Services can still bring it forward.
    _ = try launch(try appURL(app))
  }

  public func quit(app: String) throws(HandsError) {
    let running = Self.runningApps(app)
    running.forEach { $0.terminate() }
    // An app that asks to save changes stays open; the run goes on.
    _ = poll(timeout: 10) { running.allSatisfy { kill($0.processIdentifier, 0) != 0 } }
  }

  // MARK: Screen

  public func capture(app: String) throws(HandsError) -> Screenshot {
    let (image, scale, window) = try captureWindow(app)
    guard let png = Self.png(image) else { throw .Failed(reason: "The screenshot can’t be saved as PNG.") }
    let pids = Set(Self.runningApps(app).map(\.processIdentifier))
    return Screenshot(
      png: png, scale: scale, dark: Self.systemIsDark,
      secure: Accessibility.secureFields(pids: pids, window: window.bounds))
  }

  public func locate(app: String, template: Data, minSimilarity: Double) throws(HandsError) -> Point? {
    guard let source = CGImageSourceCreateWithData(template as CFData, nil),
      let image = CGImageSourceCreateImageAtIndex(source, 0, nil)
    else { throw .Failed(reason: "The image to look for can’t be read.") }
    let (shot, scale, _) = try captureWindow(app)
    guard let screen = Self.bitmap(shot) else { throw .Failed(reason: "The screenshot can’t be read.") }
    // The image may come from a display of another density: one taken on a Retina screen is
    // twice as large in pixels as the same button on a regular one, and the other way round.
    for factor in [1, scale > 1 ? scale : 0.5] {
      guard let wanted = Self.bitmap(factor == 1 ? image : Self.resized(image, by: factor) ?? image) else { continue }
      if let match = TemplateMatcher.find(wanted, in: screen, minSimilarity: minSimilarity, isCancelled: isCancelled) {
        return Point(
          x: (Double(match.x) + Double(wanted.width) / 2) / scale,
          y: (Double(match.y) + Double(wanted.height) / 2) / scale)
      }
      if isCancelled() { return nil }
    }
    return nil
  }

  // MARK: Mouse and keyboard

  public func click(app: String, at: Point, button: MouseButton, doubleClick: Bool, modifiers: Modifiers) throws(HandsError) {
    let (window, point) = try place(app, at)
    clicking(window)
    defer { clicking(nil) }
    let (down, up, mouse): (CGEventType, CGEventType, CGMouseButton) =
      button == .left ? (.leftMouseDown, .leftMouseUp, .left) : (.rightMouseDown, .rightMouseUp, .right)
    // Whatever happens on the way, the keys don't stay pressed.
    defer { release(modifiers) }
    let flags = try press(modifiers)
    let move = CGEvent(mouseEventSource: SyntheticInput.source, mouseType: .mouseMoved, mouseCursorPosition: point, mouseButton: mouse)
    move?.flags = flags
    try post(move)
    usleep(50_000)
    for count in 1...(doubleClick ? 2 : 1) {
      for type in [down, up] {
        let event = CGEvent(mouseEventSource: SyntheticInput.source, mouseType: type, mouseCursorPosition: point, mouseButton: mouse)
        event?.setIntegerValueField(.mouseEventClickState, value: Int64(count))
        event?.flags = flags
        try post(event)
        usleep(30_000)
      }
    }
  }

  public func hover(app: String, at: Point) throws(HandsError) {
    let (window, point) = try place(app, at)
    clicking(window)
    defer { clicking(nil) }
    try mouse(.mouseMoved, at: point)
    usleep(50_000)
  }

  public func drag(app: String, path: [Point], button: MouseButton, modifiers: Modifiers) throws(HandsError) {
    guard let first = path.first else { return }
    let (window, start) = try place(app, first)
    let points = path.map { CGPoint(x: window.minX + $0.x, y: window.minY + $0.y) }
    clicking(window)
    defer { clicking(nil) }
    let (down, dragged, up): (CGEventType, CGEventType, CGEventType) =
      button == .left ? (.leftMouseDown, .leftMouseDragged, .leftMouseUp) : (.rightMouseDown, .rightMouseDragged, .rightMouseUp)
    // Whatever happens on the way, the keys don't stay pressed.
    defer { release(modifiers) }
    let flags = try press(modifiers)
    try mouse(.mouseMoved, at: start, button: button, flags: flags)
    usleep(50_000)
    try mouse(down, at: start, button: button, flags: flags)
    var last = start
    // Whatever happens on the way, the button doesn't stay pressed.
    defer { try? mouse(up, at: last, button: button, flags: flags) }
    // Apps tell a drag from a click by the pause after the press and the moves that follow.
    usleep(100_000)
    for point in points.dropFirst() {
      if isCancelled() { return }
      try mouse(dragged, at: point, button: button, flags: flags)
      last = point
      usleep(10_000)
    }
    usleep(100_000)
  }

  public func scroll(app: String, at: Point, direction: ScrollDirection, amount: UInt32, unit: ScrollUnit) throws(HandsError) {
    let (window, point) = try place(app, at)
    clicking(window)
    defer { clicking(nil) }
    try mouse(.mouseMoved, at: point)
    usleep(50_000)
    // A line a turn, as a wheel does; pixels in small steps, as a trackpad does.
    let step = unit == .lines ? 1 : Self.pixelsPerScroll
    var left = Int32(clamping: amount)
    while left > 0 {
      if isCancelled() { return }
      let turn = min(step, left)
      left -= turn
      // Positive values bring what is above or to the left into sight.
      let (vertical, horizontal): (Int32, Int32) =
        switch direction {
        case .up: (turn, 0)
        case .down: (-turn, 0)
        case .left: (0, turn)
        case .right: (0, -turn)
        }
      let event = CGEvent(
        scrollWheelEvent2Source: SyntheticInput.source, units: unit == .lines ? .line : .pixel, wheelCount: 2,
        wheel1: vertical, wheel2: horizontal, wheel3: 0)
      event?.location = point
      if unit == .pixels { event?.setIntegerValueField(.scrollWheelEventIsContinuous, value: 1) }
      try post(event)
      usleep(unit == .lines ? 30_000 : 10_000)
    }
    usleep(50_000)
  }

  /// The front window of `app` and `at` in screen points; input must be possible.
  private func place(_ app: String, _ at: Point) throws(HandsError) -> (window: CGRect, point: CGPoint) {
    try requireInput()
    guard let window = Self.frontWindow(of: Self.runningApps(app)) else { throw .NoWindow(app: app) }
    return (window.bounds, CGPoint(x: window.bounds.minX + at.x, y: window.bounds.minY + at.y))
  }

  private func mouse(
    _ type: CGEventType, at point: CGPoint, button: MouseButton = .left, flags: CGEventFlags = []
  ) throws(HandsError) {
    let event = CGEvent(
      mouseEventSource: SyntheticInput.source, mouseType: type, mouseCursorPosition: point,
      mouseButton: button == .left ? .left : .right)
    if type != .mouseMoved { event?.setIntegerValueField(.mouseEventClickState, value: 1) }
    event?.flags = flags
    try post(event)
  }

  /// Presses the keys of `modifiers`; returns the flags every event sent while they are held carries.
  private func press(_ modifiers: Modifiers) throws(HandsError) -> CGEventFlags {
    var flags: CGEventFlags = []
    for code in modifiers.keyCodes {
      flags.insert(KeyStroke.flag(of: code))
      try key(code, down: true, flags: flags)
    }
    return flags
  }

  /// Lets go of the keys of `modifiers` in reverse order; keys that were never pressed stay as they are.
  private func release(_ modifiers: Modifiers) {
    var flags = modifiers.flags
    for code in modifiers.keyCodes.reversed() {
      flags.remove(KeyStroke.flag(of: code))
      try? key(code, down: false, flags: flags)
    }
  }

  public func typeText(text: String) throws(HandsError) {
    try requireInput()
    for character in text {
      if isCancelled() { return }
      switch character {
      case "\n", "\r", "\r\n": try tap(KeyStroke("return")!)
      case "\t": try tap(KeyStroke("tab")!)
      default:
        let units = Array(String(character).utf16)
        for keyDown in [true, false] {
          let event = CGEvent(keyboardEventSource: SyntheticInput.source, virtualKey: 0, keyDown: keyDown)
          event?.flags = []
          event?.keyboardSetUnicodeString(stringLength: units.count, unicodeString: units)
          try post(event)
        }
      }
      usleep(Self.keyInterval)
    }
  }

  public func pressKeys(keys: String) throws(HandsError) {
    try requireInput()
    guard let stroke = KeyStroke(keys) else { throw .Failed(reason: "“\(keys)” isn’t a key combination.") }
    try tap(stroke)
  }

  public func clipboard() throws(HandsError) -> String {
    NSPasteboard.general.string(forType: .string) ?? ""
  }

  public func setClipboard(text: String) throws(HandsError) {
    let pasteboard = NSPasteboard.general
    pasteboard.clearContents()
    guard pasteboard.setString(text, forType: .string) else {
      throw .Failed(reason: "The clipboard didn’t take the text.")
    }
  }

  public func userInputDetected() -> Bool {
    input.takeUserInput()
  }

  /// Presses modifiers, the key, and lets go in reverse order, as a person would.
  private func tap(_ stroke: KeyStroke) throws(HandsError) {
    var flags: CGEventFlags = []
    var held: [(CGKeyCode, CGEventFlags)] = []
    for code in stroke.modifiers {
      flags.insert(KeyStroke.flag(of: code))
      held.append((code, flags))
      try key(code, down: true, flags: flags)
    }
    try key(stroke.key, down: true, flags: flags)
    try key(stroke.key, down: false, flags: flags)
    for (code, flagsWhileHeld) in held.reversed() {
      try key(code, down: false, flags: flagsWhileHeld.subtracting(KeyStroke.flag(of: code)))
    }
    usleep(Self.keyInterval)
  }

  private func key(_ code: CGKeyCode, down: Bool, flags: CGEventFlags) throws(HandsError) {
    let event = CGEvent(keyboardEventSource: SyntheticInput.source, virtualKey: code, keyDown: down)
    event?.flags = flags
    try post(event)
  }

  private func post(_ event: CGEvent?) throws(HandsError) {
    guard let event else { throw .Failed(reason: "The system didn’t create an input event.") }
    event.post(tap: .cghidEventTap)
  }

  private func requireInput() throws(HandsError) {
    wakeDisplay()
    // Keys and clicks on a locked screen go to the lock screen, typed text into its password field.
    if Self.screenIsLocked { throw .ScreenLocked }
  }

  /// A sleeping display has no picture: window capture fails and clicks would only wake it.
  private func wakeDisplay() {
    guard CGDisplayIsAsleep(CGMainDisplayID()) != 0 else { return }
    var wake: IOPMAssertionID = 0
    if IOPMAssertionDeclareUserActivity("JEVision is running a test" as CFString, kIOPMUserActiveLocal, &wake) == kIOReturnSuccess {
      IOPMAssertionRelease(wake)
    }
    _ = poll(timeout: 5) { CGDisplayIsAsleep(CGMainDisplayID()) == 0 }
  }

  public static var screenIsLocked: Bool {
    let session = CGSessionCopyCurrentDictionary() as? [String: Any]
    return session?["CGSSessionScreenIsLocked"] as? Bool ?? false
  }

  // MARK: Video

  public func startVideo(app: String, path: String, hide: [Rect]) throws(HandsError) {
    guard CGPreflightScreenCaptureAccess() else { throw .PermissionDenied(permission: "screenRecording") }
    wakeDisplay()
    let video = RunVideo(app: app, url: URL(filePath: path), hide: hide)
    try video.start()
    self.video = video
  }

  public func stopVideo() throws(HandsError) {
    guard let video else { return }
    self.video = nil
    try video.stop()
  }

  // MARK: Helpers

  private func appURL(_ app: String) throws(HandsError) -> URL {
    guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: app) else { throw .AppNotFound(app: app) }
    return url
  }

  public static func runningApps(_ app: String) -> [NSRunningApplication] {
    NSRunningApplication.runningApplications(withBundleIdentifier: app).filter { !$0.isTerminated }
  }

  /// Opens or brings forward the app; returns it once it's running.
  private func launch(_ url: URL) throws(HandsError) -> NSRunningApplication? {
    let config = NSWorkspace.OpenConfiguration()
    config.activates = true
    let result = Self.wait { done in
      NSWorkspace.shared.openApplication(at: url, configuration: config) { app, error in
        done((app?.processIdentifier, error?.localizedDescription))
      }
    }
    if let error = result.1 { throw .Failed(reason: error) }
    return result.0.flatMap { NSRunningApplication(processIdentifier: $0) }
  }

  /// Waits until one of the apps shows a window. Some apps open none; the run then goes on
  /// and the next look at the screen says so.
  private func waitForWindow(_ apps: [NSRunningApplication]) {
    guard !apps.isEmpty else { return }
    _ = poll(timeout: Self.launchTimeout) { Self.frontWindow(of: apps) != nil }
  }

  /// Checks `condition` every 50 ms until it holds, the time is up or the run is cancelled.
  private func poll(timeout: TimeInterval, _ condition: () -> Bool) -> Bool {
    let deadline = Date(timeIntervalSinceNow: timeout)
    while !condition() {
      guard Date() < deadline, !isCancelled() else { return false }
      usleep(50_000)
    }
    return true
  }

  /// Runs `start` and blocks this (non-main) thread until it calls back.
  static func wait<T: Sendable>(_ start: (@escaping @Sendable (T) -> Void) -> Void) -> T {
    let box = Box<T>()
    let done = DispatchSemaphore(value: 0)
    start { value in
      box.value = value
      done.signal()
    }
    done.wait()
    return box.value!
  }

  /// The front window of any of `apps`: its id, frame in global top-left coordinates and title.
  public static func frontWindow(of apps: [NSRunningApplication]) -> (id: CGWindowID, bounds: CGRect, title: String?)? {
    front(of: windows(of: apps))
  }

  /// The first of `windows` big enough to be one a person works in. Apps put tiny helper windows
  /// in front for a moment (TextEdit a 66×20 one after typing), and a picture of those checks nothing.
  static func front(of windows: [(id: CGWindowID, bounds: CGRect, title: String?)]) -> (id: CGWindowID, bounds: CGRect, title: String?)? {
    windows.first { $0.bounds.width >= 100 && $0.bounds.height >= 40 } ?? windows.first
  }

  /// The apps' ordinary windows on screen, front to back; bounds in global points, top-left origin.
  static func windows(of apps: [NSRunningApplication]) -> [(id: CGWindowID, bounds: CGRect, title: String?)] {
    let pids = Set(apps.map(\.processIdentifier))
    guard !pids.isEmpty,
      let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
        as? [[String: Any]]
    else { return [] }
    return list.compactMap { info in
      guard let pid = info[kCGWindowOwnerPID as String] as? pid_t, pids.contains(pid),
        info[kCGWindowLayer as String] as? Int == 0,
        let id = info[kCGWindowNumber as String] as? CGWindowID,
        let dict = info[kCGWindowBounds as String] as? NSDictionary,
        let bounds = CGRect(dictionaryRepresentation: dict), bounds.width > 1, bounds.height > 1
      else { return nil }
      return (id, bounds, info[kCGWindowName as String] as? String)
    }
  }

  private func captureWindow(_ app: String) throws(HandsError) -> (
    CGImage, Double, (id: CGWindowID, bounds: CGRect, title: String?)
  ) {
    guard CGPreflightScreenCaptureAccess() else { throw .PermissionDenied(permission: "screenRecording") }
    wakeDisplay()
    return try Self.captureFrontWindow(app)
  }

  /// The app's windows above its front window and over it, front to back: tooltips, menus and
  /// popovers are windows of their own. `list` is `CGWindowListCopyWindowInfo` output.
  static func overlays(above front: CGWindowID, bounds: CGRect, pids: Set<pid_t>, in list: [[String: Any]]) -> [CGWindowID] {
    var ids: [CGWindowID] = []
    for info in list {
      guard let id = info[kCGWindowNumber as String] as? CGWindowID else { continue }
      if id == front { break }
      guard let pid = info[kCGWindowOwnerPID as String] as? pid_t, pids.contains(pid),
        let dict = info[kCGWindowBounds as String] as? NSDictionary,
        let rect = CGRect(dictionaryRepresentation: dict), rect.intersects(bounds)
      else { continue }
      ids.append(id)
    }
    return ids
  }

  /// A picture of the app's front window with whatever of the app shows over it, the window it
  /// shows, and pixels per point.
  static func captureFrontWindow(_ app: String) throws(HandsError) -> (
    image: CGImage, scale: Double, window: (id: CGWindowID, bounds: CGRect, title: String?)
  ) {
    let apps = runningApps(app)
    guard let window = frontWindow(of: apps) else { throw .NoWindow(app: app) }
    let id = window.id
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
    let above = Set(overlays(above: id, bounds: window.bounds, pids: Set(apps.map(\.processIdentifier)), in: list))
    let result = wait { (done: @escaping @Sendable (Result<Shot, HandsError>) -> Void) in
      Task {
        do {
          let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
          guard let scWindow = content.windows.first(where: { $0.windowID == id }) else {
            return done(.failure(.NoWindow(app: app)))
          }
          let config = SCStreamConfiguration()
          let filter: SCContentFilter
          let size: CGSize
          let center = CGPoint(x: window.bounds.midX, y: window.bounds.midY)
          let shown = content.windows.filter { above.contains($0.windowID) }
          if !shown.isEmpty, let display = content.displays.first(where: { $0.frame.contains(center) }) {
            // The window's part of the screen with only this app's windows on it, so other apps
            // never cover it and a tooltip or menu over it is in the picture.
            filter = SCContentFilter(display: display, including: [scWindow] + shown)
            config.sourceRect = window.bounds.offsetBy(dx: -display.frame.minX, dy: -display.frame.minY)
            config.backgroundColor = .clear
            config.ignoreShadowsDisplay = true
            size = window.bounds.size
          } else {
            filter = SCContentFilter(desktopIndependentWindow: scWindow)
            config.ignoreShadowsSingleWindow = true
            size = filter.contentRect.size
          }
          let scale = Double(filter.pointPixelScale)
          config.width = Int((size.width * CGFloat(scale)).rounded())
          config.height = Int((size.height * CGFloat(scale)).rounded())
          config.showsCursor = false
          let image = try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: config)
          done(.success(Shot(image: image, scale: scale)))
        } catch {
          // Turning the permission off while JEVision runs shows up here, not in the preflight.
          done(.failure(CGPreflightScreenCaptureAccess() ? .Failed(reason: error.localizedDescription) : .PermissionDenied(permission: "screenRecording")))
        }
      }
    }
    let shot = try result.get()
    return (shot.image, shot.scale, window)
  }

  /// Whether macOS draws windows dark. Apps that force their own appearance aren't detected.
  static var systemIsDark: Bool {
    let style = CFPreferencesCopyValue(
      "AppleInterfaceStyle" as CFString, kCFPreferencesAnyApplication, kCFPreferencesCurrentUser, kCFPreferencesAnyHost)
    return (style as? String)?.lowercased() == "dark"
  }

  static func png(_ image: CGImage) -> Data? {
    let data = NSMutableData()
    guard let destination = CGImageDestinationCreateWithData(data, UTType.png.identifier as CFString, 1, nil) else { return nil }
    CGImageDestinationAddImage(destination, image, nil)
    return CGImageDestinationFinalize(destination) ? data as Data : nil
  }

  static func resized(_ image: CGImage, by factor: Double) -> CGImage? {
    let width = max(1, Int((Double(image.width) * factor).rounded()))
    let height = max(1, Int((Double(image.height) * factor).rounded()))
    guard let space = image.colorSpace,
      let context = CGContext(
        data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0, space: space,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
    else { return nil }
    context.interpolationQuality = .high
    context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
    return context.makeImage()
  }

  /// The image's own pixel values as RGBA, without colour conversion, so a template matches the
  /// screenshot it was cut from.
  static func bitmap(_ image: CGImage) -> Bitmap? {
    let width = image.width, height = image.height
    let space = image.colorSpace.flatMap { $0.model == .rgb ? $0 : nil } ?? CGColorSpace(name: CGColorSpace.sRGB)!
    var pixels = [UInt8](repeating: 0, count: width * height * 4)
    let drawn = pixels.withUnsafeMutableBytes { buffer in
      guard
        let context = CGContext(
          data: buffer.baseAddress, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
          space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
      else { return false }
      context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
      return true
    }
    return drawn ? Bitmap(width: width, height: height, pixels: pixels) : nil
  }
}

private struct Shot: @unchecked Sendable {
  let image: CGImage
  let scale: Double
}

private final class Box<T>: @unchecked Sendable {
  var value: T?
}
