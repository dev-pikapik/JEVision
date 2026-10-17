import AppKit
import ApplicationServices
import SwiftUI

enum Permission: CaseIterable, Identifiable {
  case screenRecording, accessibility

  var id: Self { self }

  // macOS 27 renamed the Accessibility list in System Settings.
  private static var hasDeviceControl: Bool {
    ProcessInfo.processInfo.isOperatingSystemAtLeast(OperatingSystemVersion(majorVersion: 27, minorVersion: 0, patchVersion: 0))
  }

  var title: String {
    switch self {
    case .screenRecording: String(localized: "Screen Recording")
    case .accessibility:
      Self.hasDeviceControl ? String(localized: "Device Control and Data Access") : String(localized: "Accessibility")
    }
  }

  var reason: String {
    switch self {
    case .screenRecording: String(localized: "To take screenshots of the app you test")
    case .accessibility: String(localized: "To click, type and press keys for you")
    }
  }

  /// The list's name on the Privacy & Security page of System Settings.
  var listName: String {
    switch self {
    case .screenRecording: String(localized: "Screen & System Audio Recording")
    case .accessibility: title
    }
  }

  var symbol: String {
    switch self {
    case .screenRecording: "rectangle.dashed.badge.record"
    case .accessibility: "hand.point.up.left.fill"
    }
  }

  var color: Color {
    switch self {
    case .screenRecording: .purple
    case .accessibility: .blue
    }
  }

  var isGranted: Bool {
    switch self {
    case .screenRecording: CGPreflightScreenCaptureAccess()
    case .accessibility: AXIsProcessTrusted()
    }
  }

  /// Puts JEVision in the System Settings list (the first time) and opens that list.
  func request() {
    switch self {
    case .screenRecording:
      CGRequestScreenCaptureAccess()
    case .accessibility:
      // The value of kAXTrustedCheckOptionPrompt, which Swift 6 treats as unsafe shared state.
      AXIsProcessTrustedWithOptions(["AXTrustedCheckOptionPrompt": true] as CFDictionary)
    }
    openSettings()
  }

  func openSettings() {
    let anchor = self == .screenRecording ? "Privacy_ScreenCapture" : "Privacy_Accessibility"
    guard let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?\(anchor)") else { return }
    NSWorkspace.shared.open(url)
  }
}
