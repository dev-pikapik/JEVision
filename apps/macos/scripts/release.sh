#!/bin/bash
# Builds a JEVision release: the universal app with the optimized core, signed with a Developer ID
# and the hardened runtime, in build/JEVision-<version>.dmg. Notarizes and staples both the app and
# the disk image when NOTARY_PROFILE is set; without it the script stops after signing.
#   SIGN_IDENTITY="Developer ID Application: Name (TEAMID)"   required, unless ADHOC=1
#   NOTARY_PROFILE=name     a notarytool keychain profile (see "Releasing" in README.md)
#   SPARKLE_PUBLIC_ED_KEY   public key of update archives (default: the Keychain's, from generate_keys)
#   SPARKLE_KEY_ACCOUNT     Keychain account of the update key (default: jevision)
#   BUILD_NUMBER=42         CFBundleVersion (default: the number of commits on this branch)
#   ADHOC=1                 sign ad-hoc to check the build and packaging (CI); can't be shipped
# The version is the workspace version in the root Cargo.toml.
set -euo pipefail
cd "$(dirname "$0")/.."

if ! xcrun --find notarytool >/dev/null 2>&1; then
  export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
fi

ADHOC=${ADHOC:-}
NOTARY_PROFILE=${NOTARY_PROFILE:-}
SPARKLE_KEY_ACCOUNT=${SPARKLE_KEY_ACCOUNT:-jevision}
if [ -n "$ADHOC" ]; then
  if [ -n "${SIGN_IDENTITY:-}" ] || [ -n "$NOTARY_PROFILE" ]; then
    echo "ADHOC=1 can't be combined with SIGN_IDENTITY or NOTARY_PROFILE." >&2
    exit 2
  fi
elif [ -z "${SIGN_IDENTITY:-}" ]; then
  echo "Set SIGN_IDENTITY to a Developer ID Application certificate, for example:" >&2
  echo "  SIGN_IDENTITY=\"Developer ID Application: Your Name (TEAMID)\" $0" >&2
  echo "Certificates in your Keychain: security find-identity -v -p codesigning" >&2
  echo "To check the build without one, run with ADHOC=1." >&2
  exit 2
fi

# Sparkle offers an update when its build number is higher, so it must keep growing.
if [ -z "${BUILD_NUMBER:-}" ]; then
  if [ "$(git rev-parse --is-shallow-repository)" = true ]; then
    echo "This is a shallow clone, so the commit count isn't a build number: fetch the full history or set BUILD_NUMBER." >&2
    exit 2
  fi
  BUILD_NUMBER=$(git rev-list --count HEAD)
fi
export BUILD_NUMBER

swift package resolve >/dev/null
SPARKLE_BIN=.build/artifacts/sparkle/Sparkle/bin
if [ -z "${SPARKLE_PUBLIC_ED_KEY:-}" ] && [ -z "$ADHOC" ]; then
  if key=$("$SPARKLE_BIN/generate_keys" --account "$SPARKLE_KEY_ACCOUNT" -p 2>/dev/null); then
    SPARKLE_PUBLIC_ED_KEY=$key
  fi
fi
if [ -z "${SPARKLE_PUBLIC_ED_KEY:-}" ]; then
  if [ -n "$NOTARY_PROFILE" ]; then
    echo "No update key: a release without one could never update itself. Create it once with:" >&2
    echo "  $PWD/$SPARKLE_BIN/generate_keys --account $SPARKLE_KEY_ACCOUNT" >&2
    exit 2
  fi
  echo "==> No update key: this build won't look for updates"
else
  export SPARKLE_PUBLIC_ED_KEY
fi

echo "==> Building the core (release)"
PROFILE=release ../../scripts/build-core-apple.sh
echo "==> Building and signing the app"
scripts/build.sh

APP=build/JEVision.app
PLIST=$APP/Contents/Info.plist
VERSION=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$PLIST")
DMG=build/JEVision-$VERSION.dmg

echo "==> Checking signatures"
codesign --verify --deep --strict --verbose=2 "$APP"
NESTED=(
  "$APP/Contents/Helpers/jevision"
  "$APP/Contents/Frameworks/Sparkle.framework"
  "$APP/Contents/Frameworks/Sparkle.framework/Versions/B/Autoupdate"
  "$APP/Contents/Frameworks/Sparkle.framework/Versions/B/Updater.app"
  "$APP/Contents/Frameworks/Sparkle.framework/Versions/B/XPCServices/Installer.xpc"
  "$APP/Contents/Frameworks/Sparkle.framework/Versions/B/XPCServices/Downloader.xpc"
  "$APP"
)
for code in "${NESTED[@]}"; do
  info=$(codesign --display --verbose=2 "$code" 2>&1)
  if ! grep -q 'flags=.*runtime' <<<"$info"; then
    echo "Not signed with the hardened runtime: $code" >&2
    exit 1
  fi
  if [ -z "$ADHOC" ] && ! grep -qF "Authority=$SIGN_IDENTITY" <<<"$info"; then
    echo "Not signed with $SIGN_IDENTITY: $code" >&2
    exit 1
  fi
done

# Submits a file to Apple's notary service and waits; prints the log when Apple rejects it.
notarize() {
  local result id status
  result=$(xcrun notarytool submit "$1" --keychain-profile "$NOTARY_PROFILE" --wait --output-format json) || true
  status=$(plutil -extract status raw -o - - <<<"$result" 2>/dev/null) || status="not submitted: $result"
  if [ "$status" != Accepted ]; then
    echo "Notarization of $1 failed: $status" >&2
    if id=$(plutil -extract id raw -o - - <<<"$result" 2>/dev/null); then
      xcrun notarytool log "$id" --keychain-profile "$NOTARY_PROFILE" >&2 || true
    fi
    exit 1
  fi
}

if [ -n "$NOTARY_PROFILE" ]; then
  echo "==> Notarizing the app"
  ZIP=build/JEVision-notarize.zip
  ditto -c -k --keepParent "$APP" "$ZIP"
  notarize "$ZIP"
  rm "$ZIP"
  xcrun stapler staple "$APP"
  spctl --assess --type execute --verbose=2 "$APP"
fi

echo "==> Making $DMG"
STAGE=build/dmg
rm -rf "$STAGE" "$DMG"
mkdir -p "$STAGE"
ditto "$APP" "$STAGE/JEVision.app"
ln -s /Applications "$STAGE/Applications"
hdiutil create -quiet -volname "JEVision $VERSION" -srcfolder "$STAGE" -fs HFS+ -format ULFO "$DMG"
rm -rf "$STAGE"
hdiutil verify -quiet "$DMG"
if [ -z "$ADHOC" ]; then
  codesign --force --sign "$SIGN_IDENTITY" --timestamp "$DMG"
  codesign --verify --strict "$DMG"
fi

if [ -n "$NOTARY_PROFILE" ]; then
  echo "==> Notarizing the disk image"
  notarize "$DMG"
  xcrun stapler staple "$DMG"
  xcrun stapler validate "$DMG"
  spctl --assess --type open --context context:primary-signature --verbose=2 "$DMG"
elif [ -z "$ADHOC" ]; then
  echo "==> Gatekeeper (rejects the app until it is notarized):"
  spctl --assess --type execute --verbose=2 "$APP" || true
  cat <<EOF

Signed, not notarized. To notarize, store an app-specific password once
(appleid.apple.com > Sign-In and Security > App-Specific Passwords):
  xcrun notarytool store-credentials jevision --apple-id you@example.com --team-id TEAMID
then run again with NOTARY_PROFILE=jevision.
EOF
fi

echo "==> $VERSION ($BUILD_NUMBER): $PWD/$DMG"
shasum -a 256 "$DMG"
