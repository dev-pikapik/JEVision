import Foundation
import JEVisionCore

/// Texts for remote access: errors from the core and entries of the activity log.
enum RemoteText {
  static func error(_ error: any Error) -> String {
    guard let error = error as? RemoteError else { return error.localizedDescription }
    return text(error)
  }

  static func text(_ error: RemoteError) -> String {
    switch error {
    case .PairingCodeInvalid:
      return String(localized: "remote.error.pairingCodeInvalid", defaultValue: "The pairing code isn’t valid.", bundle: .app, locale: .app)
    case .PairingCodeExpired:
      return String(localized: "remote.error.pairingCodeExpired", defaultValue: "The pairing code has expired.", bundle: .app, locale: .app)
    case .PairingCodeUsed:
      return String(localized: "remote.error.pairingCodeUsed", defaultValue: "The pairing code was already used.", bundle: .app, locale: .app)
    case .PairingLocked:
      return String(
        localized: "remote.error.pairingLocked", defaultValue: "Too many wrong pairing codes. Pairing is paused for a while.", bundle: .app, locale: .app)
    case .BadSignature:
      return String(localized: "remote.error.badSignature", defaultValue: "The device couldn’t prove who it is.", bundle: .app, locale: .app)
    case .NotPaired:
      return String(localized: "remote.error.notPaired", defaultValue: "The device isn’t paired with this Mac.", bundle: .app, locale: .app)
    case .Revoked:
      return String(localized: "remote.error.revoked", defaultValue: "Access for the device was revoked.", bundle: .app, locale: .app)
    case .NotAllowed:
      return String(localized: "remote.error.notAllowed", defaultValue: "The device may only view, not run tests.", bundle: .app, locale: .app)
    case .MacLocked:
      return String(localized: "remote.error.macLocked", defaultValue: "The Mac’s screen is locked.", bundle: .app, locale: .app)
    case .RunInProgress:
      return String(localized: "remote.error.runInProgress", defaultValue: "Another test is running on the Mac.", bundle: .app, locale: .app)
    case .DeclinedOnMac:
      return String(localized: "remote.error.declinedOnMac", defaultValue: "The run wasn’t allowed on the Mac.", bundle: .app, locale: .app)
    case .HostOffline:
      return String(localized: "remote.error.hostOffline", defaultValue: "The Mac can’t be reached.", bundle: .app, locale: .app)
    case .ProjectNotFound:
      return String(localized: "remote.error.projectNotFound", defaultValue: "The project isn’t open on the Mac.", bundle: .app, locale: .app)
    case .TestNotFound:
      return String(localized: "remote.error.testNotFound", defaultValue: "The test doesn’t exist.", bundle: .app, locale: .app)
    case .TestUnreadable:
      return String(
        localized: "remote.error.testUnreadable", defaultValue: "The test can’t be read or has problems to fix first.", bundle: .app, locale: .app)
    case .RunNotFound:
      return String(localized: "remote.error.runNotFound", defaultValue: "The run doesn’t exist.", bundle: .app, locale: .app)
    case .PictureNotFound:
      return String(localized: "remote.error.pictureNotFound", defaultValue: "The picture doesn’t exist.", bundle: .app, locale: .app)
    case .PathOutsideProject:
      return String(localized: "remote.error.pathOutsideProject", defaultValue: "The file is outside the project folder.", bundle: .app, locale: .app)
    case .TooLarge:
      return String(localized: "remote.error.tooLarge", defaultValue: "The file is too large to send.", bundle: .app, locale: .app)
    case .ProtocolMismatch:
      return String(
        localized: "remote.error.protocol", defaultValue: "The device speaks another version of JEVision. Update both apps.", bundle: .app, locale: .app)
    case .OfferWrongScheme, .OfferMalformed:
      return String(localized: "remote.error.offerMalformed", defaultValue: "This isn’t a JEVision pairing code.", bundle: .app, locale: .app)
    case .InvalidSecretKey:
      return String(localized: "remote.error.invalidSecretKey", defaultValue: "The Mac’s key in the Keychain is damaged.", bundle: .app, locale: .app)
    case .InvalidRelayUrl:
      return String(
        localized: "remote.error.invalidRelayUrl", defaultValue: "The relay address isn’t valid. Use an https:// address.", bundle: .app, locale: .app)
    case .SignerFailed:
      return String(localized: "remote.error.signerFailed", defaultValue: "The device couldn’t sign in with its key.", bundle: .app, locale: .app)
    case .Network:
      return String(localized: "remote.error.network", defaultValue: "The network connection failed.", bundle: .app, locale: .app)
    case .HostNotRunning:
      return String(localized: "remote.error.hostNotRunning", defaultValue: "Remote access is off.", bundle: .app, locale: .app)
    case .Storage:
      return String(localized: "remote.error.storage", defaultValue: "The list of paired devices can’t be saved.", bundle: .app, locale: .app)
    case .HostFailed:
      return String(localized: "remote.error.hostFailed", defaultValue: "The Mac couldn’t do it. See the run on the Mac.", bundle: .app, locale: .app)
    case .DataRowNotFound:
      return String(
        localized: "remote.error.dataRowNotFound", defaultValue: "The test’s data table doesn’t have that row anymore.", bundle: .app, locale: .app)
    case .DeviceTooOld:
      return String(
        localized: "remote.error.deviceTooOld", defaultValue: "The app on the device is out of date. Update it to the latest version.", bundle: .app,
        locale: .app)
    case .MacTooOld:
      return String(
        localized: "remote.error.macTooOld", defaultValue: "JEVision on this Mac is out of date for that device. Update JEVision.", bundle: .app,
        locale: .app)
    case .Unsupported:
      return String(
        localized: "remote.error.unsupported",
        defaultValue: "The device asked for something this version of JEVision can’t do. Update JEVision on this Mac.", bundle: .app, locale: .app)
    case .DevicesFromNewer:
      return String(
        localized: "remote.error.devicesFromNewer",
        defaultValue:
          "The list of paired devices on the Mac was saved by a newer version of JEVision, so it can’t be changed. Update JEVision on the Mac.",
        bundle: .app, locale: .app)
    case .TestChanged:
      return String(
        localized: "remote.error.testChanged", defaultValue: "The test changed on the Mac in the meantime. Look at it again and repeat the change.",
        bundle: .app, locale: .app)
    case .TestBusyOnMac:
      return String(
        localized: "remote.error.testBusyOnMac", defaultValue: "The test is open on the Mac with a change that isn’t saved yet.", bundle: .app,
        locale: .app)
    case .EditInvalid:
      return String(
        localized: "remote.error.editInvalid", defaultValue: "The test can’t be changed this way. Check the value and try again.", bundle: .app,
        locale: .app)
    case .EditNeedsMac:
      return String(
        localized: "remote.error.editNeedsMac", defaultValue: "This change needs a question answered on the Mac. Make it on the Mac.", bundle: .app,
        locale: .app)
    case .Unknown:
      return String(
        localized: "remote.error.unknown", defaultValue: "A newer version of JEVision reported a problem this one doesn’t know. Update JEVision.",
        bundle: .app, locale: .app)
    }
  }

  /// The line under an activity entry: what it was about. A refusal for the app version says
  /// the version in its red line instead, and a list of devices that couldn't be read says
  /// where the copy of it is.
  static func subject(_ entry: Activity) -> String? {
    guard let subject = entry.subject else { return nil }
    switch (entry.kind, entry.error) {
    case (.devicesUnreadable, _):
      return String(
        localized: "remote.activity.unreadableCopy", defaultValue: "A copy was saved as \(subject). Pair your devices again.", bundle: .app,
        locale: .app)
    case (_, .DeviceTooOld?), (_, .MacTooOld?):
      return nil
    default:
      return subject
    }
  }

  /// The red line of an activity entry: why it failed, with the version to update to when
  /// the app version was the reason.
  static func problem(_ entry: Activity) -> String? {
    guard let error = entry.error else { return nil }
    switch (error, entry.subject) {
    case (.DeviceTooOld, let version?):
      return String(
        localized: "remote.activity.deviceTooOld", defaultValue: "The app on the device is out of date. Update it to version \(version) or later.",
        bundle: .app, locale: .app)
    case (.MacTooOld, let version?):
      return String(
        localized: "remote.activity.macTooOld",
        defaultValue: "JEVision on this Mac is out of date for that device. Update JEVision to version \(version) or later.", bundle: .app,
        locale: .app)
    default:
      return text(error)
    }
  }

  /// What happened, as the activity log says it.
  static func kind(_ kind: ActivityKind) -> String {
    switch kind {
    case .paired: String(localized: "remote.activity.paired", defaultValue: "Paired", bundle: .app, locale: .app)
    case .pairingFailed: String(localized: "remote.activity.pairingFailed", defaultValue: "Pairing failed", bundle: .app, locale: .app)
    case .signedIn: String(localized: "remote.activity.signedIn", defaultValue: "Connected", bundle: .app, locale: .app)
    case .signInFailed: String(localized: "remote.activity.signInFailed", defaultValue: "Connection refused", bundle: .app, locale: .app)
    case .runStarted: String(localized: "remote.activity.runStarted", defaultValue: "Started a run", bundle: .app, locale: .app)
    case .runStopped: String(localized: "remote.activity.runStopped", defaultValue: "Stopped a run", bundle: .app, locale: .app)
    case .testEdited: String(localized: "remote.activity.testEdited", defaultValue: "Changed a test", bundle: .app, locale: .app)
    case .reportRead: String(localized: "remote.activity.reportRead", defaultValue: "Opened a report", bundle: .app, locale: .app)
    case .denied: String(localized: "remote.activity.denied", defaultValue: "Request refused", bundle: .app, locale: .app)
    case .permissionChanged: String(localized: "remote.activity.permissionChanged", defaultValue: "Permission changed", bundle: .app, locale: .app)
    case .revoked: String(localized: "remote.activity.revoked", defaultValue: "Access revoked", bundle: .app, locale: .app)
    case .assistantCalled:
      String(localized: "remote.activity.assistantCalled", defaultValue: "Used a tool", bundle: .app, locale: .app)
    case .devicesUnreadable:
      String(localized: "remote.activity.devicesUnreadable", defaultValue: "Paired devices couldn’t be read", bundle: .app, locale: .app)
    }
  }

}
