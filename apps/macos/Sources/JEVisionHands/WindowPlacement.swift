import ApplicationServices
import CoreGraphics
import Foundation
import JEVisionCore

/// Puts the window of the app under test at a fixed size and position. What shows through a
/// translucent window, and how an app lays itself out, depend on both, so a picture taken
/// anywhere else would not match the reference.
enum WindowPlacement {
  /// Apps round to whole points; one more or less is the same window.
  static let tolerance: CGFloat = 1

  /// What the wanted size and position do not match in `frame` (global points, top-left origin):
  /// the size first. `nil` when the window is as wanted. `wanted` and `actual` are written
  /// `900 × 600` for a size and `40, 60` for a position.
  static func difference(size: WindowSize?, position: WindowPosition?, frame: CGRect) -> (what: String, wanted: String, actual: String)? {
    if let size, !close(frame.width, size.width) || !close(frame.height, size.height) {
      return ("size", "\(size.width) × \(size.height)", "\(whole(frame.width)) × \(whole(frame.height))")
    }
    if let position, !close(frame.minX, position.x) || !close(frame.minY, position.y) {
      return ("position", "\(position.x), \(position.y)", "\(whole(frame.minX)), \(whole(frame.minY))")
    }
    return nil
  }

  /// Puts the window of one of `pids` that is at `bounds` (global points) as wanted, and checks
  /// that it took it.
  static func apply(app: String, pids: Set<pid_t>, window bounds: CGRect, size: WindowSize?, position: WindowPosition?) throws(HandsError) {
    guard AXIsProcessTrusted() else { throw .PermissionDenied(permission: "accessibility") }
    guard let target = axWindow(of: pids, at: bounds) else { throw .NoWindow(app: app) }
    var asks: [(attribute: String, value: AXValue?)] = []
    if let position {
      var point = CGPoint(x: Double(position.x), y: Double(position.y))
      asks.append((kAXPositionAttribute, AXValueCreate(.cgPoint, &point)))
    }
    if let size {
      var extent = CGSize(width: Double(size.width), height: Double(size.height))
      asks.append((kAXSizeAttribute, AXValueCreate(.cgSize, &extent)))
    }
    for ask in asks {
      var settable: DarwinBoolean = false
      guard AXUIElementIsAttributeSettable(target, ask.attribute as CFString, &settable) == .success, settable.boolValue
      else { throw .WindowFixed(app: app) }
    }
    // Moving a window can shrink it to fit the screen and resizing can move it: position, size,
    // position again.
    for ask in asks + asks.prefix(1) {
      guard let value = ask.value, AXUIElementSetAttributeValue(target, ask.attribute as CFString, value) == .success
      else { throw .WindowFixed(app: app) }
    }
    // An app may animate to the new frame.
    var frame = Accessibility.frame(of: target) ?? bounds
    for _ in 0..<20 where difference(size: size, position: position, frame: frame) != nil {
      usleep(50_000)
      frame = Accessibility.frame(of: target) ?? frame
    }
    if let wrong = difference(size: size, position: position, frame: frame) {
      throw .WindowDiffers(app: app, what: wrong.what, wanted: wrong.wanted, actual: wrong.actual)
    }
  }

  private static func axWindow(of pids: Set<pid_t>, at bounds: CGRect) -> AXUIElement? {
    for pid in pids {
      let app = AXUIElementCreateApplication(pid)
      AXUIElementSetMessagingTimeout(app, 2)
      var windows: CFTypeRef?
      guard AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &windows) == .success,
        let found = (windows as? [AXUIElement])?.first(where: {
          Accessibility.frame(of: $0).map { abs($0.minX - bounds.minX) < 2 && abs($0.minY - bounds.minY) < 2 } == true
        })
      else { continue }
      return found
    }
    return nil
  }

  private static func close(_ actual: CGFloat, _ wanted: some BinaryInteger) -> Bool {
    abs(actual - CGFloat(wanted)) <= tolerance
  }

  private static func whole(_ number: CGFloat) -> Int { Int(number.rounded()) }
}
