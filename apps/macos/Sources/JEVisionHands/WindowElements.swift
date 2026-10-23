import ApplicationServices
import Foundation
import JEVisionCore

/// One element as accessibility reports it, before its frame is put in terms of the window.
struct RawElement {
  var role = ""
  var subrole = ""
  var id = ""
  var title = ""
  var description = ""
  var value = ""
  var placeholder = ""
  var enabled = true
  var selected = false
  /// In global points, top-left origin; nil when the element doesn't say where it is.
  var frame: CGRect?
}

extension Accessibility {
  /// Most elements a window is searched for; more than any real window has.
  static let elementLimit = 5000
  /// Deepest level searched, so a view that contains itself can't keep the walk going.
  static let depthLimit = 64
  /// Longest a search may take before it gives what it found by then.
  static let searchTime: TimeInterval = 3

  /// The elements of the window at `bounds` (global points, top-left origin) of one of `pids`
  /// and of the menus open in it, each before what is inside it. Empty when accessibility
  /// knows no such window, as for an app drawn without accessibility support.
  static func elements(pids: Set<pid_t>, window bounds: CGRect) -> [ElementNode] {
    let deadline = Date.now.addingTimeInterval(searchTime)
    for pid in pids {
      let app = AXUIElementCreateApplication(pid)
      AXUIElementSetMessagingTimeout(app, timeout)
      var windows: CFTypeRef?
      guard AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &windows) == .success,
        let windows = windows as? [AXUIElement],
        let window = windowRoot(windows, bounds: bounds, limit: elementLimit, depth: depthLimit, read: {
          guard Date.now < deadline, let (raw, contents) = read($0) else { return nil }
          var children = contents
          if [kAXWindowRole, kAXSheetRole].contains(raw.role) {
            var sheets: CFTypeRef?
            if AXUIElementCopyAttributeValue($0, "AXSheets" as CFString, &sheets) == .success {
              for sheet in sheets as? [AXUIElement] ?? [] where !children.contains(where: { CFEqual($0, sheet) }) {
                children.append(sheet)
              }
            }
          }
          return (raw, children)
        })
      else { continue }
      return flatten([window] + openMenus(of: app), window: bounds, limit: elementLimit, depth: depthLimit) {
        Date.now < deadline ? read($0) : nil
      }
    }
    return []
  }

  /// A captured sheet is a window of its own in screen capture, but accessibility can keep it
  /// inside its parent window. Find that sheet before mapping its controls to capture coordinates.
  static func windowRoot<Node: Hashable>(
    _ roots: [Node], bounds: CGRect, limit: Int, depth: Int,
    read: (Node) -> (RawElement, [Node])?
  ) -> Node? {
    var queue = roots.map { ($0, 0) }
    var index = 0
    var visited = Set<Node>()
    while index < queue.count, visited.count < limit {
      let (node, level) = queue[index]
      index += 1
      guard visited.insert(node).inserted, let (raw, children) = read(node) else { continue }
      if [kAXWindowRole, kAXSheetRole].contains(raw.role), let frame = raw.frame,
        abs(frame.minX - bounds.minX) < 2, abs(frame.minY - bounds.minY) < 2
      {
        return node
      }
      if level < depth { queue += children.map { ($0, level + 1) } }
    }
    return nil
  }

  /// The elements under `roots`, each before what is inside it, with frames in fractions of
  /// `window`. `read` gives an element and what is inside it, or nil to leave it out with
  /// everything inside; one without a frame is left out, though not what is inside it. Stops
  /// after `limit` elements and goes no deeper than `depth` levels.
  static func flatten<Node: Hashable>(
    _ roots: [Node], window: CGRect, limit: Int, depth: Int, read: (Node) -> (RawElement, [Node])?
  ) -> [ElementNode] {
    var found: [ElementNode] = []
    var stack = roots.reversed().map { ($0, 0) }
    var visited = Set<Node>()
    while let (node, level) = stack.popLast(), found.count < limit {
      guard visited.insert(node).inserted, let (raw, children) = read(node) else { continue }
      if let f = raw.frame, window.width > 0, window.height > 0 {
        found.append(
          ElementNode(
            role: raw.role, subrole: raw.subrole, id: raw.id, title: raw.title, description: raw.description,
            // A password never leaves the app, not even into a failure message.
            value: raw.subrole == kAXSecureTextFieldSubrole ? "" : raw.value, placeholder: raw.placeholder,
            enabled: raw.enabled, selected: raw.selected,
            frame: Rect(
              x: (f.minX - window.minX) / window.width, y: (f.minY - window.minY) / window.height,
              width: f.width / window.width, height: f.height / window.height)))
      }
      if level < depth {
        stack += children.reversed().map { ($0, level + 1) }
      }
    }
    return found
  }

  /// Menus open in the app: a context menu, or the menu of a menu bar item that is open.
  private static func openMenus(of app: AXUIElement) -> [AXUIElement] {
    var menus: [AXUIElement] = []
    for child in children(of: app) {
      switch string(kAXRoleAttribute, of: child) {
      case kAXMenuRole:
        menus.append(child)
      case kAXMenuBarRole:
        // Menu bar menus are there even when closed; only an open one is part of what is shown.
        for item in children(of: child) where bool(kAXSelectedAttribute, of: item) == true {
          menus += children(of: item).filter { string(kAXRoleAttribute, of: $0) == kAXMenuRole }
        }
      default:
        break
      }
    }
    return menus
  }

  /// The attributes `read` asks for, in one round trip to the app.
  private static let attributes = [
    kAXRoleAttribute, kAXSubroleAttribute, kAXIdentifierAttribute, kAXTitleAttribute, kAXDescriptionAttribute,
    kAXValueAttribute, kAXPlaceholderValueAttribute, kAXEnabledAttribute, kAXSelectedAttribute,
    kAXPositionAttribute, kAXSizeAttribute, kAXChildrenAttribute, kAXTitleUIElementAttribute,
  ]

  private static func read(_ element: AXUIElement) -> (RawElement, [AXUIElement])? {
    var values: CFArray?
    guard
      AXUIElementCopyMultipleAttributeValues(element, attributes as CFArray, AXCopyMultipleAttributeOptions(), &values)
        == .success,
      let values = values as? [AnyObject], values.count == attributes.count
    else { return nil }
    func text(_ i: Int) -> String { values[i] as? String ?? "" }
    var raw = RawElement(
      role: text(0), subrole: text(1), id: text(2), title: text(3), description: text(4),
      value: valueText(values[5]), placeholder: text(6), enabled: (values[7] as? Bool) ?? true,
      selected: (values[8] as? Bool) ?? false)
    if let origin = axValue(values[9], .cgPoint, CGPoint.zero), let size = axValue(values[10], .cgSize, CGSize.zero) {
      raw.frame = CGRect(origin: origin, size: size)
    }
    // A field labelled by text next to it. An element's own description, such as a permission
    // status image saying “Allowed”, takes precedence over the label of the surrounding row.
    if raw.title.isEmpty, raw.description.isEmpty, CFGetTypeID(values[12]) == AXUIElementGetTypeID() {
      raw.title = string(kAXValueAttribute, of: values[12] as! AXUIElement) ?? ""
    }
    return (raw, values[11] as? [AXUIElement] ?? [])
  }

  /// A value as text: a field's text, `1` for a checked checkbox, a slider's number.
  private static func valueText(_ value: AnyObject) -> String {
    switch value {
    case let text as String: text
    case let number as NSNumber where CFGetTypeID(number) != AXValueGetTypeID(): number.stringValue
    case let url as URL: url.absoluteString
    default: ""
    }
  }

  private static func axValue<T: BitwiseCopyable>(_ raw: AnyObject, _ type: AXValueType, _ empty: T) -> T? {
    guard CFGetTypeID(raw) == AXValueGetTypeID() else { return nil }
    var out = empty
    return AXValueGetValue(raw as! AXValue, type, &out) ? out : nil
  }

  private static func children(of element: AXUIElement) -> [AXUIElement] {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, kAXChildrenAttribute as CFString, &value) == .success else { return [] }
    return value as? [AXUIElement] ?? []
  }

  private static func string(_ attribute: String, of element: AXUIElement) -> String? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, attribute as CFString, &value) == .success else { return nil }
    return value as? String
  }

  private static func bool(_ attribute: String, of element: AXUIElement) -> Bool? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, attribute as CFString, &value) == .success else { return nil }
    return value as? Bool
  }
}
