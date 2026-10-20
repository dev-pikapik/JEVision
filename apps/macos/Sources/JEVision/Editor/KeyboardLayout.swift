import Carbon.HIToolbox
import Foundation
import Observation
import Synchronization

/// The keyboard layout in use, so a key is written as what is printed on it for this person: the
/// file names keys by their place on a US keyboard, which on another layout carries another letter.
/// Next to it, the layout macOS reads ⌘ shortcuts from: the latest Latin one. Views that show a key
/// combination redraw when the layout is switched.
final class ActiveKeyboard: Observable, Sendable {
  static let shared = ActiveKeyboard()

  private let registrar = ObservationRegistrar()
  private let current = Mutex<(layout: Data?, latin: Data?)>((nil, nil))

  /// The layout's data for `UCKeyTranslate`; `nil` before `start()`.
  var layout: Data? {
    registrar.access(self, keyPath: \.layout)
    return current.withLock { $0.layout }
  }

  /// The data of the Latin layout behind ⌘ shortcuts, which macOS spells in Latin letters on any
  /// layout (⌘V in the Edit menu on a Russian one); `nil` before `start()`.
  var latinLayout: Data? {
    registrar.access(self, keyPath: \.latinLayout)
    return current.withLock { $0.latin }
  }

  /// Reads the layout now and follows its changes. Called once, at launch, on the main thread.
  func start() {
    update()
    DistributedNotificationCenter.default().addObserver(
      forName: Notification.Name(kTISNotifySelectedKeyboardInputSourceChanged as String), object: nil, queue: .main
    ) { [weak self] _ in self?.update() }
  }

  private func update() {
    let layout = KeyboardLayout.data(of: TISCopyCurrentKeyboardLayoutInputSource().takeRetainedValue())
    let latin = KeyboardLayout.data(of: TISCopyCurrentASCIICapableKeyboardLayoutInputSource().takeRetainedValue())
    registrar.withMutation(of: self, keyPath: \.layout) {
      registrar.withMutation(of: self, keyPath: \.latinLayout) { current.withLock { $0 = (layout, latin) } }
    }
  }
}

enum KeyboardLayout {
  /// The layout of an input source; `nil` for one that has none, like a drawing pad.
  static func data(of source: TISInputSource) -> Data? {
    guard let property = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) else { return nil }
    return Unmanaged<CFData>.fromOpaque(property).takeUnretainedValue() as Data
  }

  /// What the layout prints on the key with this key code, without modifiers; `nil` when it prints
  /// nothing a person could read.
  static func character(keyCode: UInt16, layout: Data) -> String? {
    layout.withUnsafeBytes { bytes in
      guard let table = bytes.baseAddress?.assumingMemoryBound(to: UCKeyboardLayout.self) else { return nil }
      var deadKeys: UInt32 = 0
      var length = 0
      var buffer = [UniChar](repeating: 0, count: 4)
      let status = UCKeyTranslate(
        table, keyCode, UInt16(kUCKeyActionDisplay), 0, UInt32(LMGetKbdType()),
        OptionBits(kUCKeyTranslateNoDeadKeysBit), &deadKeys, buffer.count, &length, &buffer)
      guard status == noErr, length > 0 else { return nil }
      let text = String(utf16CodeUnits: buffer, count: length)
      return text.unicodeScalars.allSatisfy { $0.value > 0x20 && $0.value != 0x7F } ? text : nil
    }
  }
}
