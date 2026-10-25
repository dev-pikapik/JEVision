import SwiftUI

extension RemoteError {
  var title: String {
    switch self {
    case .macOffline: String(localized: "Can’t Reach the Mac")
    case .macLocked: String(localized: "The Mac Is Locked")
    case .runInProgress: String(localized: "A Test Is Already Running")
    case .notAllowed: String(localized: "Not Allowed")
    case .declinedOnMac: String(localized: "Declined on the Mac")
    case .invalidCode: String(localized: "This Code Doesn’t Work")
    case .codeExpired: String(localized: "This Code Has Expired")
    case .revoked: String(localized: "This iPhone Was Removed")
    case .dataRowNotFound: String(localized: "This Row Is Gone")
    case .testChanged: String(localized: "The Test Changed on the Mac")
    case .testBusyOnMac: String(localized: "The Test Is Being Changed on the Mac")
    case .editInvalid: String(localized: "This Change Doesn’t Work")
    case .editNeedsMac: String(localized: "Make This Change on the Mac")
    case .tooLarge: String(localized: "Too Much for the iPhone")
    case .updateApp: String(localized: "Update JEVision Remote")
    case .updateMac: String(localized: "Update JEVision on the Mac")
    case .other: String(localized: "Something Went Wrong")
    }
  }

  var message: String {
    switch self {
    case .macOffline:
      String(localized: "The Mac may be asleep or offline, or JEVision isn’t open on it. Wake it up and try again.")
    case .macLocked:
      String(localized: "A test needs the Mac’s screen. Unlock the Mac and try again.")
    case .runInProgress:
      String(localized: "Wait until the test running on the Mac finishes, then try again.")
    case .notAllowed:
      String(localized: "The Mac doesn’t allow this iPhone to do that. To allow it, change this iPhone’s permission in JEVision on the Mac.")
    case .declinedOnMac:
      String(localized: "Someone at the Mac said no to this run.")
    case .invalidCode:
      String(localized: "The code was already used or the Mac stopped showing it. Show a new code on the Mac and scan it.")
    case .codeExpired:
      String(localized: "Pairing codes work for five minutes. Show a new code on the Mac and scan it.")
    case .revoked:
      String(localized: "The Mac no longer knows this iPhone. Forget the Mac here and pair again.")
    case .dataRowNotFound:
      String(localized: "The test’s data table changed on the Mac. Pull down to refresh the test and choose a row again.")
    case .testChanged:
      String(localized: "Someone changed it on the Mac while you were editing. The test now shows what the Mac has; make your change again.")
    case .testBusyOnMac:
      String(localized: "The test is open on the Mac with a change that isn’t saved yet. Finish that change on the Mac, then try again.")
    case .editInvalid:
      String(localized: "The Mac can’t take this value or this change. Check what you entered and try again.")
    case .editNeedsMac:
      String(localized: "This change needs a question answered on the Mac. Open the test in JEVision on the Mac to make it.")
    case .tooLarge:
      String(localized: "It’s more than the Mac sends to or takes from an iPhone. Make the text shorter, or do this on the Mac.")
    case .updateApp(let version?):
      String(localized: "This Mac needs JEVision Remote \(version) or later. Update the app and try again.")
    case .updateApp(nil):
      String(localized: "This Mac uses a newer JEVision. Update JEVision Remote and try again.")
    case .updateMac(let version?):
      String(localized: "This iPhone needs JEVision \(version) or later on the Mac. Update JEVision on the Mac and try again.")
    case .updateMac(nil):
      String(localized: "This iPhone needs a newer JEVision on the Mac. Update JEVision on the Mac to the latest version and try again.")
    case .other:
      String(localized: "Try again. If it keeps happening, restart JEVision on the Mac.")
    }
  }

  var symbol: String {
    switch self {
    case .macOffline: "wifi.slash"
    case .macLocked: "lock.desktopcomputer"
    case .runInProgress: "hourglass"
    case .notAllowed, .revoked: "hand.raised"
    case .declinedOnMac: "xmark.circle"
    case .dataRowNotFound: "tablecells"
    case .testChanged: "arrow.triangle.2.circlepath"
    case .testBusyOnMac: "pencil.and.list.clipboard"
    case .editInvalid: "exclamationmark.circle"
    case .editNeedsMac, .tooLarge: "desktopcomputer"
    case .invalidCode, .codeExpired: "qrcode"
    case .updateApp: "arrow.down.app"
    case .updateMac: "desktopcomputer.and.arrow.down"
    case .other: "exclamationmark.triangle"
    }
  }

  /// For a pairing code from a newer JEVision, which this version can't read.
  static var newerCodeMessage: String {
    String(localized: "This code is from a newer JEVision. Update JEVision Remote to pair with this Mac.")
  }
}

/// A screen-filling error with what happened, what to do, and a way to try again.
struct ErrorView: View {
  let error: RemoteError
  var retry: (() async -> Void)?

  var body: some View {
    ContentUnavailableView {
      Label(error.title, systemImage: error.symbol)
    } description: {
      Text(error.message)
    } actions: {
      if let retry {
        Button("Try Again") { Task { await retry() } }
          .buttonStyle(.glassProminent)
      }
    }
  }
}

/// What a screen has from the Mac so far.
enum Loadable<Value> {
  case loading
  case loaded(Value)
  case failed(RemoteError)

  var value: Value? {
    if case .loaded(let value) = self { value } else { nil }
  }
}

/// Shows a loaded value, a spinner, or the error with a retry.
struct LoadableView<Value, Content: View>: View {
  let state: Loadable<Value>
  let retry: () async -> Void
  @ViewBuilder let content: (Value) -> Content

  var body: some View {
    switch state {
    case .loading: ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
    case .loaded(let value): content(value)
    case .failed(let error): ErrorView(error: error, retry: retry)
    }
  }
}
