# App Privacy answers for JEVision Remote

What to answer in App Store Connect ▸ JEVision Remote ▸ **App Privacy**, question by question,
and the code each answer rests on. Check the evidence again before every release that changes
what the app sends or stores: a new network call, a new SDK or a new file can change an answer.
Apple's definitions are on
[App privacy details on the App Store](https://developer.apple.com/app-store/app-privacy-details/).

## Privacy Policy

| Field | Answer |
| --- | --- |
| Privacy Policy URL | `https://github.com/dev-pikapik/JEVision/blob/main/docs/privacy.md` |
| User Privacy Choices URL | leave empty: there is nothing to opt out of |

## Data Collection

**Do you or your third-party partners collect data from this app?** — **No, we do not collect
data from this app.** The questionnaire then ends and the App Store shows *Data Not Collected*.

Apple counts data as collected when it leaves the device and the developer or a partner can
access it for longer than it takes to serve the request in real time. Nothing in the app does
that:

- **The only network code is the connection to the user's own Mac.**
  `apps/ios/JEVisionRemote/Remote/CoreRemoteClient.swift` calls the Rust core's
  `RemoteConnection.pair` and `.connect`, and nothing else in the app opens a connection: there is
  no `URLSession`, no web view, no SDK. The About links in Settings open GitHub in Safari.
- **What goes to the Mac belongs to the user.** Pairing sends the name typed on the pairing
  screen (`Screens/PairView.swift`, default `UIDevice.current.name`) and the public half of the
  device key (`Identity/DeviceKey.swift`). After that the iPhone sends only requests. The Mac is
  the user's computer, not the developer's, so this is not collection.
- **The relay and the address lookup serve each request in real time.** When the Mac can't be
  reached directly, iroh's relay servers (run by n0) forward the traffic, which is end-to-end
  encrypted (QUIC/TLS between the iPhone and the Mac), and the address lookup returns the Mac's
  current relay. The iPhone publishes only its relay URL, under a network key that is random for
  each launch (`Network(relay: .default, localDiscovery: false, globalDiscovery: true)` in
  `CoreRemoteClient.swift`); it identifies neither the person nor the device.
- **What stays on the iPhone is not collected**: the paired Macs in `PairedMacs.json`
  (`Remote/HostStore.swift`), the lock timeout in `UserDefaults` (`App/AppLock.swift`) and the
  Secure Enclave key (`Identity/DeviceKey.swift`). Pictures and reports are kept in memory only.
- **The camera** feeds VisionKit's `DataScannerViewController` (`Screens/ScannerView.swift`),
  which reads the QR code on the device; no frame is saved or sent.
- **The example** opened by *See an Example* (`Remote/ExampleMac.swift`) is built into the app
  and uses no network.
- The privacy manifest `apps/ios/JEVisionRemote/PrivacyInfo.xcprivacy` declares
  `NSPrivacyTracking` false and no collected data types, which matches these answers.

## Data types, one by one

If App Store Connect asks for each type anyway, or to double-check the answer above:

| Data type | Collected? | Why not |
| --- | --- | --- |
| Contact Info (name, email, phone, address) | No | No account, no form; the device name goes only to the user's Mac |
| Health and Fitness | No | Not used |
| Financial Info | No | No purchases, no payments |
| Location (precise or coarse) | No | No location API; the relay forwards packets and keeps no location |
| Sensitive Info | No | Not used |
| Contacts | No | Not used |
| User Content (photos, videos, audio, gameplay, support, other) | No | Camera frames stay on the iPhone; test pictures come from the user's Mac and are not stored |
| Browsing History | No | No web view |
| Search History | No | No search sent anywhere |
| Identifiers (User ID, Device ID) | No | The device key and the iPhone's ID live on the iPhone and the user's Mac; the network key is random per launch |
| Purchases | No | No in-app purchases |
| Usage Data (product interaction, advertising, other) | No | No analytics, no logging off the device |
| Diagnostics (crash data, performance, other) | No | The app sends no diagnostics; crash reports that iOS shares with users' consent are Apple's service, not code in the app |
| Other Data | No | — |

## Tracking

**Does this app track users?** — **No.** There is no advertising, no IDFA
(`AppTrackingTransparency` is not linked), no data broker, and no data is combined with other
companies' data. `NSPrivacyTracking` is false and `NSPrivacyTrackingDomains` is empty.

## When an answer must change

- A new SDK, analytics, crash reporter or any server of the project: the answers above stop being
  true. Update this page, the privacy policy (`docs/privacy.md`) and the privacy manifest
  together, then the questionnaire.
- A change to what pairing sends or what the iPhone stores: update *What goes to your Mac* or
  *What stays on your iPhone* in `docs/privacy.md` too.
