# Releasing JEVision

How a version of JEVision for Mac gets from `main` to a user's Mac. The list of things to tick
off for one release is [release-checklist.md](release-checklist.md); this page explains the
parts behind it.

## How it fits together

```
Cargo.toml  ──►  scripts/version.sh  ──►  core, macOS app, iPhone app, `jevision --version`
   (version)           (version + build number)

tag v1.2.3  ──►  .github/workflows/release.yml
                   1. scripts/check-version.sh  (tag = version = lock file = changelog)
                   2. the whole of ci.yml       (tests)
                   3. apps/macos/scripts/release.sh   build, sign, notarize, staple, check, .dmg,
                                                      debug symbols, signed update feed
                   4. a DRAFT GitHub release: .dmg, .dmg.sha256, .dSYMs.zip, the update feeds
                              │
                  someone publishes the draft
                              ▼
   users:  dmg download · brew install --cask · Sparkle (JEVision ▸ Check for Updates…)
```

Nothing becomes public until a person publishes the draft release.

## The version

There is one source: `version` under `[workspace.package]` in the root `Cargo.toml`.

- **Version** (`CFBundleShortVersionString`, `jevision --version`): `scripts/version.sh version`.
- **Build number** (`CFBundleVersion`): `BUILD_NUMBER` if it is set, otherwise the number of commits
  on the branch (`scripts/version.sh build`). It only ever grows, which is what Sparkle compares to
  decide that an update is newer, so release from a full clone, not a shallow one.
- The iPhone app is stamped with the same two values by a build phase
  (`apps/ios/scripts/stamp-version.sh`).
- A beta is a version such as `1.2.0-beta.1` (also `-alpha.N` and `-rc.N`); see *Beta channel*.
- `scripts/check-version.sh [vX.Y.Z]` fails when the lock file does not match, when `CHANGELOG.md`
  or `CHANGELOG.ru.md` has no section for the version, or when the tag is a different version. CI
  runs it on every push; the release workflow runs it with the tag.
- `JEVISION_VERSION=1.2.0-beta.1` makes `version.sh` give another version without editing
  `Cargo.toml`, for the update test bench only: `check-version.sh` refuses to run with it, so a
  release can't be built with it.

To change the version, edit `Cargo.toml`, run `cargo update --workspace` and move the
`[Unreleased]` notes of `CHANGELOG.md` and of `CHANGELOG.ru.md` under the new version heading (the
notes become the text of the GitHub release and of the update dialog, in English or in Russian
after the language of the Mac).

## Secrets and variables

All of them belong to the repository environment **release** (Settings ▸ Environments), so that
only the release workflow can read them. The environment can also require a reviewer for every
run. This page never lists values; keep them in a password manager.

| Name | Kind | What it is |
| --- | --- | --- |
| `DEVELOPER_ID_CERT_P12` | secret | The *Developer ID Application* certificate and its private key, exported from Keychain Access as a `.p12` file, then `base64 -i cert.p12 \| pbcopy` |
| `DEVELOPER_ID_CERT_PASSWORD` | secret | The password you chose when exporting the `.p12` |
| `NOTARY_API_KEY_P8` | secret | The App Store Connect API key (`AuthKey_XXXX.p8`) for notarization, the whole file as text |
| `NOTARY_API_KEY_ID` | secret | The key's ID, 10 characters |
| `NOTARY_API_ISSUER_ID` | secret | The issuer ID shown on the App Store Connect API keys page |
| `SPARKLE_PRIVATE_KEY` | secret | The private update key, as `generate_keys -x` writes it |
| `SPARKLE_PUBLIC_ED_KEY` | variable | The matching public key (44 characters of base64); not secret |
| `APP_STORE_CONNECT_KEY` | secret | iPhone app, optional (all six iPhone rows or none; without them the app is archived unsigned and not validated): an App Store Connect API key (`AuthKey_XXXX.p8`), the whole file as text; the notarization key will do |
| `APP_STORE_CONNECT_KEY_ID` | secret | iPhone app: that key's ID, 10 characters |
| `APP_STORE_CONNECT_ISSUER_ID` | secret | iPhone app: the issuer ID shown on the App Store Connect API keys page |
| `IOS_DISTRIBUTION_CERT_P12` | secret | iPhone app: the *Apple Distribution* certificate and its private key, exported from Keychain Access as a `.p12` file, then `base64 -i cert.p12 \| pbcopy` |
| `IOS_DISTRIBUTION_CERT_PASSWORD` | secret | iPhone app: the password you chose when exporting that `.p12` |
| `IOS_APP_STORE_PROFILE` | secret | iPhone app: the *App Store Connect* provisioning profile of `io.github.dev-pikapik.jevision.remote`, downloaded from the developer site, then `base64 -i profile.mobileprovision \| pbcopy` |

The workflow stops at its first step, before anything is built, if one of them is missing, and
names the missing ones.

The App Store Connect API key is created at *App Store Connect ▸ Users and Access ▸ Integrations ▸
App Store Connect API*. The *Developer* role is enough for notarization.

## The update key

Sparkle installs an update only when its EdDSA signature matches the public key inside the app
(`SUPublicEDKey`, written by `build.sh` from `SPARKLE_PUBLIC_ED_KEY`, and verified before the archive
is opened). Whoever holds the private key can ship code to every user, so:

- Create it once, on your Mac: `swift package resolve --package-path apps/macos`, then
  `apps/macos/.build/artifacts/sparkle/Sparkle/bin/generate_keys --account jevision`. This puts the
  key in your login keychain and prints the public key.
- Save a copy that is not on your Mac: `generate_keys --account jevision -x jevision-sparkle.key`
  writes the private key to a file; put it in a password manager and delete the file.
- **If the key is lost, installed copies can never update again**: a new key means a new
  `SUPublicEDKey`, and the apps already out there trust only the old one. Those users have to
  download the new version by hand.
- **If the key leaks**, treat it as a security incident (see [SECURITY.md](../../SECURITY.md)):
  rotate it, ship a version with the new public key signed with the old key, and say so in the
  release notes.
- Never commit the key, never paste it in an issue or a chat. The repository has no copy of it.

For the tag workflow the private key is also a repository secret (`SPARKLE_PRIVATE_KEY`). If you
would rather keep it only on your Mac, release locally (below) and do not use the workflow.

## Notarization profile (local releases)

`release.sh` notarizes with a `notarytool` keychain profile. Create it once; the password never
goes into a script or a file:

```sh
xcrun notarytool store-credentials jevision --apple-id <Apple ID> --team-id <Team ID>
```

`notarytool` asks for an app-specific password (made at
[appleid.apple.com](https://appleid.apple.com)) and stores it in the login keychain.
`xcrun notarytool history --keychain-profile jevision` shows that it works.

## Releasing from your Mac

```sh
git switch main && git pull           # a clean checkout: release.sh refuses uncommitted changes
SIGN_IDENTITY="Developer ID Application: <Name> (<Team ID>)" NOTARY_PROFILE=jevision \
  RELEASE_TAG=v<version> SPARKLE_PUBLIC_ED_KEY=<public key> \
  apps/macos/scripts/release.sh
packaging/homebrew/update-cask.sh apps/macos/build/JEVision-<version>.dmg
```

`release.sh` stops at the first problem and does these steps: clean checkout, the version matches
`RELEASE_TAG`, universal build with the optimized core, every nested binary signed with the hardened
runtime and a timestamp, the app has exactly one entitlement and the tool none (Sparkle's downloader keeps its own), a disk
image, notarization of the app and of the disk image, `stapler staple` and `stapler validate`,
`spctl --assess` on the app and on the disk image, `JEVision-<version>.dmg.sha256`, the debug
symbols (below) and the signed update feeds `appcast.xml` and `appcast-beta.xml` in
`apps/macos/build/appcast/` (`make-appcast.sh`, which `release.sh` runs last, when the disk image can
no longer change; see *Where the updates come from*).

Without `NOTARY_PROFILE` it stops after signing, which is enough to check a build. `ADHOC=1` builds
the same disk image with no certificate at all (CI does it on every pull request); such a build
cannot be shipped.

Then create the release by hand: tag, draft release with the disk image, its `.sha256`, the
`.dSYMs.zip` and `apps/macos/build/appcast/appcast.xml`, and copy `packaging/homebrew/jevision.rb`
into `Casks/` of the tap. When the release is published, replace the beta feed (see *Beta
channel*):

```sh
gh release upload appcast-beta apps/macos/build/appcast/appcast-beta.xml --clobber
```

## Releasing with the workflow

```sh
git switch main && git pull
git tag -s v<version> -m "JEVision <version>"   # or: git tag v<version>
git push origin v<version>
```

The pushed tag starts `release.yml`. When it is green, open *Releases*, check the draft (files,
notes), and press **Publish release**. The *Actions* page keeps the same files as the `release`
artifact for 90 days.

## Entitlements and permissions

The app runs with the hardened runtime and one entitlement, kept in
`apps/macos/Resources/JEVision.entitlements`:

| Entitlement | Why |
|---|---|
| `com.apple.security.automation.apple-events` | AppleScript blocks send Apple events to other apps. Without it the hardened runtime refuses them. The system still asks the user once per target app, with the text of `NSAppleEventsUsageDescription`. |

The `jevision` tool has none. Sparkle's downloader keeps the entitlements Sparkle signs it with.
`release.sh` fails when the app has a different set. Adding an entitlement means adding a row to
this table with the reason, in the same change.

Not asked for on purpose: no camera or microphone (the app records the screen of the app under
test, which needs the user's *Screen Recording* permission, not an entitlement), no *Disable Library
Validation* (only ad-hoc builds get it, since they have no team to validate against), no network
entitlements (the app is not sandboxed).

## Debug symbols

The app and `jevision` are built with debug information, which `build.sh` moves into
`build/dSYMs/JEVision.app.dSYM` and `build/dSYMs/jevision.dSYM` (both architectures) and strips
from the shipped binaries. `release.sh` checks that each dSYM carries the UUIDs of the binary it
belongs to and zips them as `JEVision-<version>.dSYMs.zip`, which becomes an asset of the release
next to the disk image. Keep it for as long as the version is in use: a crash report can only be
turned into function names and lines with the dSYMs of the build that crashed.

```sh
# a frame of a crash report: the image's load address, then the frame address
atos -arch arm64 -o JEVision.app.dSYM/Contents/Resources/DWARF/JEVision -l <load address> <address>
```

## Where the updates come from

The app asks `https://github.com/dev-pikapik/JEVision/releases/latest/download/appcast.xml`
(`SUFeedURL` in `apps/macos/Resources/Info.plist`) when it opens and once a day
(`SUEnableAutomaticChecks`, `SUScheduledCheckInterval`), and when someone chooses **JEVision ▸ Check
for Updates…** or **Settings ▸ Updates ▸ Check Now**. The update window offers **Install Update**,
**Remind Me Later** and **Skip This Version**, and shows the release notes of the Mac's language.
GitHub redirects *latest* to the newest **published, non-prerelease** release, so:

- a draft is invisible to users until you publish it, which is the point of the draft step;
- a release marked *pre-release* is skipped too;
- each release carries its own `appcast.xml` that lists earlier versions as well, because
  `make-appcast.sh` starts from the feed that is currently published.

The Homebrew cask uses the same file for `livecheck`.

Each new entry gets the release notes in English and in Russian (`<description>` and
`<description xml:lang="ru">`, from `CHANGELOG.md` and `CHANGELOG.ru.md`), the data format of the
version (see *Data format and backups*) and the EdDSA signature of the disk image. Then the feed
itself is signed (a `sparkle-signatures` comment at its end). `verify-appcast.swift`, run by
`make-appcast.sh`, checks all of it against the public key inside the app.

### What a user's Mac accepts

- **Only a signed feed.** `build.sh` sets `SURequireSignedFeed`: a feed that is unsigned, changed
  after signing or signed with another key is refused as a whole ("The update feed is improperly
  signed"), so nobody between GitHub and the user can add, remove or rewrite an entry.
- **Only a signed archive.** `SUVerifyUpdateBeforeExtraction`: the disk image must match the
  signature of its entry before Sparkle opens it.
- **Never an older version.** Sparkle offers only an entry whose `sparkle:version` (the build number)
  is higher than the installed `CFBundleVersion`. A feed with older entries only, a replayed old
  feed included, gives "You're up to date". The build number grows with every commit (*The
  version*), so `1.2.0-beta.1` built before `1.2.0` is also older than it.

`apps/macos/scripts/update-e2e.sh` checks each of these against real builds (*Testing updates
locally*).

## Beta channel

**Settings ▸ Updates ▸ Get beta versions** (off by default, `updatesIncludeBetas`) makes the app read
the beta feed, `JEVisionBetaFeedURL` in `Info.plist`:
`https://github.com/dev-pikapik/JEVision/releases/download/appcast-beta/appcast-beta.xml`, and allow
Sparkle's channel `beta`. The beta feed lists the releases (no channel) and the betas
(`<sparkle:channel>beta</sparkle:channel>`); the release feed has no betas at all, and an app
without the setting doesn't accept the channel either. Turning the setting off doesn't go back to
the release: the beta stays until a release with a higher build number replaces it.

The beta feed can't live under `releases/latest`, which skips pre-releases, so it is the only asset
of one fixed pre-release, tagged `appcast-beta`, that is replaced on every release and every beta.
Create it once:

```sh
gh release create appcast-beta --prerelease --latest=false --title "Beta update feed" \
  --notes "The update feed of the beta channel. Don't delete." apps/macos/build/appcast/appcast-beta.xml
```

`make-appcast.sh` puts a beta (a version with `-alpha.N`, `-beta.N` or `-rc.N`) into
`appcast-beta.xml` only, with the channel `beta`; a release into both feeds, without a channel. It
starts each feed from the published one, or from a folder given as its second argument.

To ship a beta: set the version in `Cargo.toml` to, say, `1.2.0-beta.1`, add its section to both
changelogs and push the tag `v1.2.0-beta.1`. The workflow drafts the release as a **pre-release**
(any version with a `-` is one, never *latest*) with the disk image, its checksum, the debug symbols
and `appcast-beta.xml`; there is no `appcast.xml`. After you publish the draft, upload the feed to
`appcast-beta` as above. Don't update the Homebrew cask for a beta. For a release, the draft has
both feeds: publish it, then upload `appcast-beta.xml` to `appcast-beta`; `appcast.xml` is already
where the app looks for it.

## Data format and backups

`JEVisionDataFormat` in `apps/macos/Resources/Info.plist` is the version of the way the app stores
tests and settings; `make-appcast.sh` copies it from the built app into each entry
(`<jevision:dataFormat>`). Before Sparkle downloads an update whose data format is higher than the
installed app's, `UpdateDataGuard` copies JEVision's folder in Application Support (without its
backups), the settings (`Settings.plist`) and the open project (without `.git` and `.jevision`)
into `~/Library/Application Support/JEVision/Backups/before-<version>-<date>`, so the old version can
still open them. If the copy fails, the update isn't installed and the user sees why. JEVision never
deletes a backup; **Settings ▸ Updates ▸ Show in Finder** opens the folder.

When a version changes what it writes (a new entry in `MIGRATIONS` and a higher `FORMAT_VERSION`
in `crates/jevision-core`, or a settings change an older app can't read), raise
`JEVisionDataFormat` in the same pull request. A unit test fails while it is lower than the core's
`FORMAT_VERSION`.

## Testing updates locally

```sh
apps/macos/.build/artifacts/sparkle/Sparkle/bin/generate_keys --account jevision-bench   # once
apps/macos/.build/artifacts/sparkle/Sparkle/bin/generate_keys --account jevision-bench -x bench.key
SPARKLE_ED_KEY_FILE=bench.key SPARKLE_PUBLIC_ED_KEY=<the printed public key> \
  apps/macos/scripts/update-e2e.sh
```

Use a key pair of its own for the bench, never the release key. The script builds JEVision as
`<version>-beta.1` (build 2) and as `<version>` (build 3), makes their disk images and feeds with
`make-appcast.sh`, serves them at `http://localhost:8765` (`PORT`) and checks with
`update-probe.swift` that the beta is offered the release with its notes in English and in Russian
and its data format, that the release isn't offered itself or the older beta, that the beta channel
is used only when it is turned on, and that a feed that was changed, isn't signed or is signed with
another key is refused. It doesn't open JEVision; at the end it prints how to try the update window
by hand and how to stop the server. Everything goes to `build/update-e2e/`.

The test copies read the feed from `JEVISION_UPDATE_FEED`. The app takes that address from the
environment it was opened with, only when it is an `http` or `https` address, and never stores it.
This doesn't weaken anything: the feed must still be signed with the key inside the app and every
archive with the same key, and no older version is accepted, so the variable can only point the
app at a copy of what the project signed. Whoever can set the environment of a user's app can
already run any program as that user.

## The Homebrew tap

`brew install --cask dev-pikapik/jevision/jevision` needs a repository
`dev-pikapik/homebrew-jevision` with the cask in `Casks/jevision.rb`. After each release,
`update-cask.sh` fills the version and the checksum of the disk image; commit the file to the tap.

The cask says `auto_updates true`: the app updates itself through Sparkle, so `brew upgrade` skips
it unless it is named (`brew upgrade --cask jevision`) or `--greedy` is given, and `brew outdated`
doesn't list it. Its `livecheck` reads the release feed, so it never sees a beta; don't update the
cask for a beta.

## Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| `release.sh`: *uncommitted changes* | Release from a clean checkout; `ADHOC=1` is the only exception |
| `check-version.sh`: *does not match* | `Cargo.toml`, `Cargo.lock`, `CHANGELOG.md` and the tag must agree; see *The version* |
| Notarization *Invalid* | `xcrun notarytool log <id> --keychain-profile jevision` lists the file and the reason; usually a nested binary that is not signed with the hardened runtime and a timestamp |
| `spctl` says *rejected* | The app was not stapled or not notarized; run `xcrun stapler validate JEVision.app` |
| Users see no update | The release is still a draft or a pre-release, or the build number is not higher than theirs |
| *The signature … doesn't match the update key* | The disk image was changed after `make-appcast.sh`, or it was signed with another key than the one in `SPARKLE_PUBLIC_ED_KEY` |
| Users see "The update feed is improperly signed" | The published feed was edited after `make-appcast.sh`, or signed with another key; upload the feed from `apps/macos/build/appcast/` again |
| Beta testers see no beta | `appcast-beta.xml` of the `appcast-beta` pre-release wasn't replaced, or the beta's build number isn't higher than theirs |
| `make-appcast.sh`: *lacks JEVisionDataFormat* | The disk image was built before the key was added to `Info.plist`; build it again |
