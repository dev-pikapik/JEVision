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
      String(localized: "This iPhone can only look at tests. To run them from here, allow it in JEVision on the Mac.")
    case .declinedOnMac:
      String(localized: "Someone at the Mac said no to this run.")
    case .invalidCode:
      String(localized: "The code was already used or the Mac stopped showing it. Show a new code on the Mac and scan it.")
    case .codeExpired:
      String(localized: "Pairing codes work for five minutes. Show a new code on the Mac and scan it.")
    case .revoked:
      String(localized: "The Mac no longer knows this iPhone. Forget the Mac here and pair again.")
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
    case .invalidCode, .codeExpired: "qrcode"
    case .other: "exclamationmark.triangle"
    }
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
