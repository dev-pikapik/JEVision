# JEVision Remote for iPhone

JEVision Remote turns an iPhone into a remote for JEVision on your Mac. Once paired, you can:

- browse the projects and tests on each paired Mac,
- see a test drawn as its flow, with its last results,
- run a test (if the Mac allows it) and follow it block by block, or stop it,
- read reports with a one-phrase reason for each failure, and open screenshots only when you need them,
- look through a project's run history.

Tests, pictures and secrets never leave the Mac. The iPhone asks for what you open and keeps
nothing but the list of paired Macs.

## Requirements

- Xcode 27 or later
- iOS 26 or later (the app uses Liquid Glass)
- Rust (install from [rustup.rs](https://rustup.rs)): the app's network layer is the JEVision core
- JEVision on the Mac, with pairing turned on in Settings → iPhone & Remote

## Build for the Simulator

No Apple developer account is needed:

```sh
xcodebuild -project apps/ios/JEVisionRemote.xcodeproj -scheme JEVisionRemote \
  -destination 'platform=iOS Simulator,name=iPhone 18 Pro' CODE_SIGNING_ALLOWED=NO build
```

Run the unit tests the same way, with `test` in place of `build`.

The first build also builds the Rust core: the JEVisionMobile target runs
`scripts/build-core.sh`, which calls `scripts/build-mobile-apple.sh` at the repository root
whenever `Frameworks/JEVisionMobileFFI.xcframework` or `Generated/JEVisionMobile.swift` is
missing or older than the Rust code. Both are generated and not committed.

A build with `CODE_SIGNING_ALLOWED=NO` can't use the keychain, so it can't make the device key
and pairing fails. To pair in the Simulator, sign the build for the Simulator only, which also
needs no account:

```sh
xcodebuild -project apps/ios/JEVisionRemote.xcodeproj -scheme JEVisionRemote \
  -destination 'platform=iOS Simulator,name=iPhone 18 Pro' \
  CODE_SIGN_IDENTITY=- CODE_SIGN_STYLE=Manual DEVELOPMENT_TEAM= build
```

The Simulator has no camera. To pair a Mac, copy the pairing link on the Mac and use
**Paste** on the first screen, or open the link with
`xcrun simctl openurl booted 'jevision://pair?...'`.

### Test against a real Mac

One test pairs with a Mac, browses its projects, tests and runs, and signs in again. It is
skipped unless asked for. Turn on pairing on the Mac, copy the pairing link and run, within
the five minutes the link works:

```sh
TEST_RUNNER_JEVISION_REMOTE_E2E=1 TEST_RUNNER_JEVISION_REMOTE_OFFER='jevision://pair?...' \
xcodebuild -project apps/ios/JEVisionRemote.xcodeproj -scheme JEVisionRemote \
  -destination 'platform=iOS Simulator,name=iPhone 18 Pro' \
  CODE_SIGN_IDENTITY=- CODE_SIGN_STYLE=Manual DEVELOPMENT_TEAM= \
  -only-testing:JEVisionRemoteTests/RemoteEndToEndTests test
```

The test leaves a device named "JEVision end-to-end test" on the Mac; remove it there afterwards.

## Run on an iPhone

1. Open `apps/ios/JEVisionRemote.xcodeproj` in Xcode.
2. In the JEVisionRemote target, open **Signing & Capabilities** and choose your team. To use
   your own Apple ID, also change the bundle identifier to one you own.
3. Connect the iPhone, select it as the run destination and press Run.

The iPhone needs a passcode: the app uses it, or Face ID, to unlock and to guard the device key.

### Reaching the Mac

The pairing code holds the Mac's addresses and its relay. After every sign-in the iPhone saves
the Mac's current addresses, looks the Mac up by its id through the n0 address service when
they change, and falls back to the relay, which forwards only encrypted data. Finding Macs by
mDNS on the local network would need the `com.apple.developer.networking.multicast`
entitlement, which Apple grants per team, so the app doesn't use it. Connecting to a Mac on the
same Wi-Fi still asks for the local network permission once.

## Privacy model

- **Pairing.** The Mac shows a QR code that is valid for five minutes and works once. Scanning
  it and confirming with Face ID or the passcode adds the Mac to this iPhone. The Mac decides
  whether the iPhone may only view tests or also run them, and can remove it at any time.
- **Device key.** On first use the iPhone creates a P-256 key in the Secure Enclave. It cannot be
  copied off the device, needs the owner's presence to sign, and is not backed up. The Mac
  knows the iPhone only by its public key. In the Simulator, which has no Secure Enclave, a
  software key in the Keychain stands in for it.
- **Connection.** Every request goes over an end-to-end encrypted connection to the Mac, directly
  or through a relay that cannot read it.
- **App lock.** The app asks for Face ID or the passcode when opened, after a chosen time in the
  background (immediately, 1, 5 or 15 minutes) and at least every 8 hours. Locking drops every
  connection, and the app screen is covered in the app switcher.
- **Storage.** Only the list of paired Macs (name, identity and addresses) is saved, in a file
  protected by iOS data protection. Forgetting a Mac deletes it from the list; to also revoke
  the iPhone, remove it in JEVision on the Mac.

## Code layout

| Folder | What it holds |
| --- | --- |
| `App/` | The app entry point, the app model, the lock and the run progress logic |
| `Remote/` | The `RemoteClient` protocol, the data types, pairing links and saved Macs |
| `Identity/` | The device key |
| `Screens/` | The SwiftUI screens |
| `Design/` | Shared views, block and verdict styles, and the wording of errors and failures |

`Remote/CoreRemoteClient.swift` is the only file that talks to the JEVision core, through the
JEVisionMobile framework target built from the generated UniFFI binding. The rest of the app
sees only `RemoteClient` and `MacSession`.
