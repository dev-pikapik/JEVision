# Encryption and the App Store

`apps/ios/JEVisionRemote/Info.plist` sets `ITSAppUsesNonExemptEncryption` to `YES`. This page
explains why, and what to answer when App Store Connect asks about a build. It is the reasoning
behind the setting, not legal advice.

## What JEVision Remote encrypts

- Everything between the iPhone and the Mac goes over iroh: QUIC (the `noq` crates) secured with
  TLS 1.3 from `rustls` on `ring`. That means AES-GCM and ChaCha20-Poly1305 for the data, X25519
  for key exchange, Ed25519 for the identities of the iPhone and the Mac, SHA-2 and HMAC. Relay
  connections use the same TLS.
- It protects the content itself: tests, run results, screenshots. It is not limited to
  authentication, copy protection or any other purpose the export rules exclude.
- Every algorithm is a published standard (IETF RFC 8446, 9000, 7748, 8032, 8439). None is
  proprietary.
- The app implements them in its Rust core. It does not only call the encryption built into
  Apple's operating system, such as `URLSession`.

## Why the value is YES

Apple wants `NO` only when an app uses no encryption, only the operating system's encryption, or
encryption exempt from export documentation. None of these fits:

- The Category 5, Part 2 exclusions of the US Export Administration Regulations (EAR) do not
  apply: the encryption is not limited to authentication, and Note 4 does not cover an app whose
  primary function is to send and receive information.
- What remains is mass-market encryption with standard algorithms: ECCN 5D992.c under EAR
  §740.17(b)(1). Since the BIS rule of 29 March 2021, such software needs no self-classification
  report, and its public source code (MIT, on GitHub) needs no notification either (§742.15(b)).
  Neither changes Apple's question, which is about the app containing non-exempt encryption.

## Answers for App Store Connect

Without an `ITSEncryptionExportComplianceCode`, App Store Connect asks for every build:

| Question | Answer |
|---|---|
| Does your app use encryption? | Yes |
| Does it qualify for an exemption in Category 5, Part 2? | No |
| Proprietary or non-standard algorithms? | No |
| Standard algorithms instead of, or in addition to, Apple's operating system? | Yes |
| Available in France? | Decide before the first release, see below |

France regulates encryption that keeps data confidential. If the app is offered there, Apple asks
for the French encryption declaration (ANSSI) and its receipt. Either file it and upload the
receipt, or leave France out of the countries the app is sold in. If Apple then issues an export
compliance code, add it to `Info.plist` as `ITSEncryptionExportComplianceCode`, and builds stop
asking.

Check these answers again whenever the core changes how it encrypts, such as a new TLS or crypto
library, or any algorithm that is not a published standard.

## The Mac app

JEVision for Mac is not sold through the App Store, so App Store Connect never asks. Its download
falls under the same mass-market classification, with no report to file.
