# Release checklist

Copy this list into an issue or a note for every release and tick it off. How the parts work, and
what each secret is, is in [releasing.md](releasing.md). A box that cannot be ticked stops the
release; do not tag around it.

## Once, before the first release

These need your Apple account or your repository settings, so no script does them for you.

- [ ] A *Developer ID Application* certificate is in the login keychain
      (`security find-identity -v -p codesigning` lists it).
- [ ] The notarization profile exists: `xcrun notarytool store-credentials jevision --apple-id
      <Apple ID> --team-id <Team ID>`, and `xcrun notarytool history --keychain-profile jevision`
      answers without an error.
- [ ] The update key exists and is backed up outside your Mac (*The update key* in
      [releasing.md](releasing.md)). Its public half is in the variable `SPARKLE_PUBLIC_ED_KEY`.
- [ ] For the tag workflow: the environment **release** exists with the secrets of the table in
      [releasing.md](releasing.md), and requires your approval for a run.
- [ ] *Private vulnerability reporting* is on (Settings ▸ Code security), because
      [SECURITY.md](../../SECURITY.md) sends reporters there, and the response times promised in it
      are ones you can keep.
- [ ] The Homebrew tap `dev-pikapik/homebrew-jevision` exists with a `Casks` folder. Without it,
      remove the Homebrew line from `CHANGELOG.md` and the Homebrew install from `README.md`.

## Before the tag

- [ ] `main` has everything of this release and CI is green on its last commit.
- [ ] The version is right everywhere: `scripts/check-version.sh` prints no error, and
      `CHANGELOG.md` and `CHANGELOG.ru.md` have a dated section for it that says in plain words what
      the user gets.
- [ ] The full gate is green on the commit you will tag:
      `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test --workspace`, `PROFILE=debug scripts/build-core-apple.sh`,
      `swift test --package-path build/apple/JEVisionCore`,
      `python3 apps/macos/scripts/check-localization.py`,
      `swift build --package-path apps/macos`, `swift test --package-path apps/macos --no-parallel`, and the
      iPhone app builds for the simulator (`xcodebuild -project apps/ios/JEVisionRemote.xcodeproj
      -scheme JEVisionRemote -destination 'generic/platform=iOS Simulator' ARCHS=arm64 build`).
- [ ] The self-test is green: `scripts/selftest.sh` (needs an unlocked screen and the permissions
      Screen Recording and Accessibility for the built app). Do not release on a red or a skipped run.
- [ ] A build with your certificate and no notarization passes: `SIGN_IDENTITY="Developer ID
      Application: <Name> (<Team ID>)" apps/macos/scripts/release.sh`.

## Manual check on the signed build

Do this on the app from that build, in a fresh macOS user account if you can, so that nothing from
earlier runs helps it.

- [ ] It opens with no Gatekeeper warning once notarized (`spctl -a -vv JEVision.app` says
      *accepted, source=Notarized Developer ID*).
- [ ] The first start shows the onboarding, and each permission it asks for can be given.
- [ ] A test of the bundled example project runs from the window, and the same test runs from the
      command line: `JEVision.app/Contents/Helpers/jevision run <project>`.
- [ ] A block that runs AppleScript against another app (Finder, for example) works: the system asks
      once to allow the control, and after *Allow* the block runs. A shell command block works too.
      If AppleScript is refused without a question, the entitlement
      `com.apple.security.automation.apple-events` did not make it into the signature.
- [ ] *JEVision ▸ About JEVision* shows the right version, the licenses open, and the links work.
- [ ] *JEVision ▸ Check for Updates…* reaches the feed and says it is up to date (or offers the new
      build, if a newer release exists).
- [ ] The update test bench passes: `apps/macos/scripts/update-e2e.sh` with the bench key prints
      *All checks passed* (*Testing updates locally* in [releasing.md](releasing.md)).
- [ ] `jevision --version` prints the version of the app.

## Tag and publish

- [ ] Tag the commit that passed the gate and push the tag (`git tag v<version>`,
      `git push origin v<version>`), or build and release from your Mac as in
      [releasing.md](releasing.md).
- [ ] The workflow is green, and the draft release has `JEVision-<version>.dmg`, its `.sha256`,
      `JEVision-<version>.dSYMs.zip` and `appcast.xml`, and the notes of the changelog.
- [ ] Download the disk image from the draft in a browser, check it
      (`shasum -a 256 -c JEVision-<version>.dmg.sha256`), open it and run the app: this is what a
      user's Mac sees, quarantine flag included.
- [ ] Press **Publish release**. Make sure it is not marked as a pre-release, or the update feed
      cannot find it.
- [ ] `curl -sIL https://github.com/dev-pikapik/JEVision/releases/latest/download/appcast.xml`
      ends with `200`.
- [ ] Replace the beta feed: `gh release upload appcast-beta apps/macos/build/appcast/appcast-beta.xml
      --clobber` (the first time, create the pre-release: *Beta channel* in [releasing.md](releasing.md)),
      then `curl -sIL https://github.com/dev-pikapik/JEVision/releases/download/appcast-beta/appcast-beta.xml`
      ends with `200`.
- [ ] Update the cask: `packaging/homebrew/update-cask.sh JEVision-<version>.dmg`, commit
      `jevision.rb` to `Casks/` in the tap, then `brew install --cask dev-pikapik/jevision/jevision`
      installs the new version.
- [ ] From the previous release (if there is one): *Check for Updates…* offers the new version and
      installs it.

## After

- [ ] `CHANGELOG.md` and `CHANGELOG.ru.md` have an empty `[Unreleased]` section on top of the new
      version.
- [ ] The version in `Cargo.toml` is not changed again until the next release is planned.
- [ ] The contents of the release are not edited afterwards: a mistake is fixed by a new version.

## iPhone app on the App Store

JEVision Remote ships on the App Store on the same day as the Mac release, with the same version
(`scripts/version.sh`); its build number is the commit count. Its texts, the notes for App Review
and the limits they must fit are in [`apps/ios/fastlane`](../../apps/ios/fastlane/README.md).

### Once, before the first submission

The steps of *What only the account owner can do* below are done.

### Before the tag

App Review usually answers in one or two days, and a rejection costs a round more: submit about a
week before the date of the Mac release.

- [ ] `python3 apps/ios/scripts/check-metadata.py` prints `Metadata: OK`, and `release_notes.txt`
      in `en-US` and `ru` says what the iPhone part of the version's section in `CHANGELOG.md` says.
- [ ] `apps/ios/scripts/archive.sh` ends with *The archive is fine* on the commit you will tag:
      the unsigned archive has the right bundle ID, version and build, the icon, the privacy
      manifest, the encryption answer and both languages.
- [ ] If a screen changed, take the screenshots again: `apps/ios/scripts/screenshots.sh <folder>`
      makes them for the 6.9" and 6.5" iPhones in English and Russian.
- [ ] Upload from that commit, with the API key of *What only the account owner can do*:
      `DEVELOPMENT_TEAM=… PROVISIONING_PROFILE="…" APP_STORE_CONNECT_KEY_ID=…
      APP_STORE_CONNECT_ISSUER_ID=… APP_STORE_CONNECT_KEY_FILE=… apps/ios/scripts/archive.sh upload`.
      It validates the app first and uploads only a valid one. Apple sends an email when the build
      has been processed, usually within an hour. A build number can be uploaded once: a second
      upload needs a newer commit.
- [ ] In App Store Connect ▸ JEVision Remote, add the iOS version `scripts/version.sh` prints, and
      fill in English and Russian from `apps/ios/fastlane/metadata`: promotional text, description,
      keywords, support URL, marketing URL and *What's New* (`release_notes.txt`; the first
      version has no such field). Name, subtitle and privacy policy URL are under *App Information*.
- [ ] Screenshots for both languages: the 6.9" set, and the 6.5" set if App Store Connect asks.
- [ ] The build that was uploaded is chosen for the version, and the encryption question, if it
      comes up, is answered as in [app-store-encryption.md](app-store-encryption.md).
- [ ] *App Review Information*: *Sign-in required* is off, the notes are the text of
      `apps/ios/fastlane/metadata/review_information/notes.txt`, and the contact is filled in.
- [ ] *Version Release* is *Manually release this version*, so that the app waits for the Mac.
- [ ] *App Privacy* still matches [app-store-privacy-answers.md](app-store-privacy-answers.md);
      if the release changed what the app sends or stores, update the page and the answers first.
- [ ] *Add for Review*, then *Submit to App Review*.

### On the release day

- [ ] The tag workflow's job *iPhone app for the App Store* is green and validated the app. The
      notice *The App Store secrets are not set* means it only checked the unsigned archive.
- [ ] The version is *Pending Developer Release*. Once the Mac release is published, press
      *Release This Version*; the App Store shows it within a day.
- [ ] `docs/iphone.html` (*Get the app*), `README.md` and `apps/ios/README.md` say where to get the
      app from the App Store instead of building it with Xcode, with the App Store link.
- [ ] The App Store page shows both languages, the app installs on an iPhone, *See an Example*
      works, and the iPhone pairs with the released Mac app.

### If App Review rejects the app

1. Read the message in App Store Connect ▸ JEVision Remote ▸ *App Review*. It names a guideline
   and usually says what the reviewer saw.
2. If the reviewer missed something (for example, that the app needs the user's own Mac, or how to
   try it without one), answer in the same thread: point to *See an Example* and to the review
   notes, and add what was missing to `review_information/notes.txt` for the next time. No new
   build is needed; the review continues.
3. If the app must change, fix it in a branch like any bug, merge it, and upload a build from the
   new commit with `apps/ios/scripts/archive.sh upload`. Choose the new build for the same version
   and submit again. Change the texts in `apps/ios/fastlane/metadata` and in App Store Connect
   together.
4. If you think the guideline is applied wrongly, appeal to the App Review Board from the same
   page, and say so in the thread.
5. The Mac and the iPhone ship together: if the iPhone app is not approved in time, move the date
   of the release for both.

### What only the account owner can do

These need your Apple account; no script and no workflow does them, and nothing in the repository
creates certificates, profiles or keys.

1. **Accept Apple's agreements.** Sign in to [developer.apple.com](https://developer.apple.com/account)
   and to [App Store Connect](https://appstoreconnect.apple.com). If either shows a banner about a
   new or updated agreement, read it and accept it; until then, uploads are refused. A free app
   needs no paid-apps agreement, bank account or tax forms.
2. **Register the app's ID**, unless Xcode already did: developer site ▸ *Certificates,
   Identifiers & Profiles* ▸ *Identifiers* ▸ **+** ▸ *App IDs* ▸ *App*, explicit bundle ID
   `io.github.dev-pikapik.jevision.remote`, description *JEVision Remote*, no extra capabilities.
   This ID never changes.
3. **Create the app's page.** App Store Connect ▸ *Apps* ▸ **+** ▸ *New App*: platform iOS, name
   *JEVision Remote*, primary language *English (U.S.)*, the bundle ID above, SKU
   `jevision-remote`. Then in *App Information*: category *Developer Tools*, add the *Russian*
   localization, and copy name, subtitle and privacy policy URL from `apps/ios/fastlane/metadata`.
4. **Make an API key**, or reuse the one for notarization: App Store Connect ▸ *Users and Access* ▸
   *Integrations* ▸ *App Store Connect API* ▸ *Team Keys* ▸ **+**, role *Developer*. Download the
   `.p8` file at once (Apple shows it only once), and write down the *Key ID* and the *Issuer ID*
   above the list.
5. **Make the signing certificate and profile.** In Xcode ▸ *Settings* ▸ *Accounts* ▸ your team ▸
   *Manage Certificates* ▸ **+** ▸ *Apple Distribution*. Then on the developer site ▸ *Profiles* ▸
   **+** ▸ *App Store Connect*, choose the app's ID and that certificate, name the profile (for
   example *JEVision Remote App Store*) and download it. To export the certificate for GitHub,
   find *Apple Distribution: …* in Keychain Access ▸ *My Certificates*, choose *Export* as a
   `.p12` file and set a password.
6. **Give GitHub the six secrets** in Settings ▸ Environments ▸ **release**, as the table in
   [releasing.md](releasing.md) says. Without them the tag workflow still archives and checks the
   app, but App Store Connect does not validate it.
7. **Answer the questionnaires** of the app's page, once:
   - *App Privacy*: as in [app-store-privacy-answers.md](app-store-privacy-answers.md), then
     *Publish*.
   - *Age Rating*: none of the content it asks about is in the app, so every answer is *None* or
     *No*, which rates it 4+.
   - *Pricing and Availability*: free, in every country you want.
   - *Digital Services Act* (App Store Connect ▸ *Business*): say whether you are a trader; the
     App Store shows the app in the European Union only after this answer.
   - *App Review Information*: your name, phone and email, which Apple uses only when the reviewer
     needs help.
