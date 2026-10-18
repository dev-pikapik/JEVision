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

  /// The service's name for `tccutil`.
  var service: String {
    switch self {
    case .screenRecording: "ScreenCapture"
    case .accessibility: "Accessibility"
    }
  }

  /// Makes macOS forget what it remembers about this copy of JEVision for the permission — an
  /// entry left by an older build whose switch is on but no longer works — then asks again.
  /// Only JEVision's own entry is reset.
  func reset() async {
    if let bundleID = Bundle.main.bundleIdentifier {
      let tccutil = Process()
      tccutil.executableURL = URL(filePath: "/usr/bin/tccutil")
      tccutil.arguments = ["reset", service, bundleID]
      tccutil.standardOutput = FileHandle.nullDevice
      tccutil.standardError = FileHandle.nullDevice
      await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
        tccutil.terminationHandler = { _ in done.resume() }
        do { try tccutil.run() } catch { done.resume() }
      }
    }
    request()
  }

  func openSettings() {
    let anchor = self == .screenRecording ? "Privacy_ScreenCapture" : "Privacy_Accessibility"
    guard let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?\(anchor)") else { return }
    NSWorkspace.shared.open(url)
  }
}
