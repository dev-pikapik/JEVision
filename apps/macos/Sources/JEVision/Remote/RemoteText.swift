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
      return String(localized: "remote.error.pairingCodeInvalid", defaultValue: "The pairing code isn’t valid.")
    case .PairingCodeExpired:
      return String(localized: "remote.error.pairingCodeExpired", defaultValue: "The pairing code has expired.")
    case .PairingCodeUsed:
      return String(localized: "remote.error.pairingCodeUsed", defaultValue: "The pairing code was already used.")
    case .PairingLocked:
      return String(
        localized: "remote.error.pairingLocked", defaultValue: "Too many wrong pairing codes. Pairing is paused for a while.")
    case .BadSignature:
      return String(localized: "remote.error.badSignature", defaultValue: "The device couldn’t prove who it is.")
    case .NotPaired:
      return String(localized: "remote.error.notPaired", defaultValue: "The device isn’t paired with this Mac.")
    case .Revoked:
      return String(localized: "remote.error.revoked", defaultValue: "Access for the device was revoked.")
    case .NotAllowed:
      return String(localized: "remote.error.notAllowed", defaultValue: "The device may only view, not run tests.")
    case .MacLocked:
      return String(localized: "remote.error.macLocked", defaultValue: "The Mac’s screen is locked.")
    case .RunInProgress:
      return String(localized: "remote.error.runInProgress", defaultValue: "Another test is running on the Mac.")
    case .DeclinedOnMac:
      return String(localized: "remote.error.declinedOnMac", defaultValue: "The run wasn’t allowed on the Mac.")
    case .HostOffline:
      return String(localized: "remote.error.hostOffline", defaultValue: "The Mac can’t be reached.")
    case .ProjectNotFound:
      return String(localized: "remote.error.projectNotFound", defaultValue: "The project isn’t open on the Mac.")
    case .TestNotFound:
      return String(localized: "remote.error.testNotFound", defaultValue: "The test doesn’t exist.")
    case .TestUnreadable:
      return String(
        localized: "remote.error.testUnreadable", defaultValue: "The test can’t be read or has problems to fix first.")
    case .RunNotFound:
      return String(localized: "remote.error.runNotFound", defaultValue: "The run doesn’t exist.")
    case .PictureNotFound:
      return String(localized: "remote.error.pictureNotFound", defaultValue: "The picture doesn’t exist.")
    case .PathOutsideProject:
      return String(localized: "remote.error.pathOutsideProject", defaultValue: "The file is outside the project folder.")
    case .TooLarge:
      return String(localized: "remote.error.tooLarge", defaultValue: "The file is too large to send.")
    case .ProtocolMismatch:
      return String(
        localized: "remote.error.protocol", defaultValue: "The device speaks another version of JEVision. Update both apps.")
    case .OfferWrongScheme, .OfferMalformed:
      return String(localized: "remote.error.offerMalformed", defaultValue: "This isn’t a JEVision pairing code.")
    case .InvalidSecretKey:
      return String(localized: "remote.error.invalidSecretKey", defaultValue: "The Mac’s key in the Keychain is damaged.")
    case .InvalidRelayUrl:
      return String(
        localized: "remote.error.invalidRelayUrl", defaultValue: "The relay address isn’t valid. Use an https:// address.")
    case .SignerFailed:
      return String(localized: "remote.error.signerFailed", defaultValue: "The device couldn’t sign in with its key.")
    case .Network:
      return String(localized: "remote.error.network", defaultValue: "The network connection failed.")
    case .HostNotRunning:
      return String(localized: "remote.error.hostNotRunning", defaultValue: "Remote access is off.")
    case .Storage:
      return String(localized: "remote.error.storage", defaultValue: "The list of paired devices can’t be saved.")
    case .HostFailed:
      return String(localized: "remote.error.hostFailed", defaultValue: "The Mac couldn’t do it. See the run on the Mac.")
    case .DataRowNotFound:
      return String(
        localized: "remote.error.dataRowNotFound", defaultValue: "The test’s data table doesn’t have that row anymore.")
    }
  }

  /// What happened, as the activity log says it.
  static func kind(_ kind: ActivityKind) -> String {
    switch kind {
    case .paired: String(localized: "remote.activity.paired", defaultValue: "Paired")
    case .pairingFailed: String(localized: "remote.activity.pairingFailed", defaultValue: "Pairing failed")
    case .signedIn: String(localized: "remote.activity.signedIn", defaultValue: "Connected")
    case .signInFailed: String(localized: "remote.activity.signInFailed", defaultValue: "Connection refused")
    case .runStarted: String(localized: "remote.activity.runStarted", defaultValue: "Started a run")
    case .runStopped: String(localized: "remote.activity.runStopped", defaultValue: "Stopped a run")
    case .reportRead: String(localized: "remote.activity.reportRead", defaultValue: "Opened a report")
    case .denied: String(localized: "remote.activity.denied", defaultValue: "Request refused")
    case .permissionChanged: String(localized: "remote.activity.permissionChanged", defaultValue: "Permission changed")
    case .revoked: String(localized: "remote.activity.revoked", defaultValue: "Access revoked")
    }
  }

}
