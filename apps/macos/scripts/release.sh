#!/bin/bash
# Builds a JEVision release: the universal app with the optimized core, signed with a Developer ID
# and the hardened runtime, in build/JEVision-<version>.dmg. Notarizes and staples both the app and
# the disk image when NOTARY_PROFILE is set; without it the script stops after signing.
#   SIGN_IDENTITY="Developer ID Application: Name (TEAMID)"   required, unless ADHOC=1
#   NOTARY_PROFILE=name     a notarytool keychain profile (see docs/maintainers/releasing.md)
#   SPARKLE_PUBLIC_ED_KEY   public key of update archives (default: the Keychain's, from generate_keys)
#   SPARKLE_KEY_ACCOUNT     Keychain account of the update key (default: jevision)
#   BUILD_NUMBER=42         CFBundleVersion (default: the number of commits on this branch)
#   RELEASE_TAG=v1.0.0      the tag being released: fails unless it is this version (CI sets it)
#   ADHOC=1                 sign ad-hoc to check the build and packaging (CI); can't be shipped
#   SPARKLE_ED_KEY_FILE     file with the private update key (CI); default: the key in the Keychain
# The version is the workspace version in the root Cargo.toml (scripts/version.sh). Next to the
# disk image the script writes JEVision-<version>.dmg.sha256, the debug symbols as
# JEVision-<version>.dSYMs.zip and, when it can sign one, the update feed build/appcast/appcast.xml
# (make-appcast.sh). A real release is built from a clean checkout: uncommitted changes stop it.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$(cd ../.. && pwd)

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
fi
BUILD_NUMBER=$("$ROOT/scripts/version.sh" build)
export BUILD_NUMBER
"$ROOT/scripts/check-version.sh" ${RELEASE_TAG:+"$RELEASE_TAG"}
if [ -z "$ADHOC" ] && [ -n "$(git status --porcelain)" ]; then
  echo "There are uncommitted changes. Release from a clean checkout of the commit that gets the tag:" >&2
  git status --short >&2
  exit 2
fi
# A wrong profile name should fail now, not after the build.
if [ -n "$NOTARY_PROFILE" ] && ! xcrun notarytool history --keychain-profile "$NOTARY_PROFILE" >/dev/null 2>&1; then
  echo "No working notarytool profile called $NOTARY_PROFILE: see docs/maintainers/releasing.md." >&2
  exit 2
fi

# Package.swift points at the core package, so it is built before SwiftPM resolves anything.
echo "==> Building the core (release, from the locked sources)"
rm -rf "$ROOT/build/apple"
PROFILE=release ../../scripts/build-core-apple.sh
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

echo "==> Building and signing the app"
JEVISION_BUILD_OUTPUT=build scripts/build.sh

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

# The app and its tool run with the hardened runtime and the fewest entitlements: the app has the one
# in Resources/JEVision.entitlements, the tool none. Anything else was added by mistake; ad-hoc
# builds also carry the one that lets them load Sparkle.
if [ -z "$ADHOC" ]; then
  entitlements() { codesign --display --entitlements - --xml "$1" 2>/dev/null | python3 -c '
import plistlib, sys
data = sys.stdin.buffer.read()
print(" ".join(sorted(plistlib.loads(data))) if data else "")'; }
  expected=$(plutil -convert json -o - Resources/JEVision.entitlements | python3 -c 'import json, sys; print(" ".join(sorted(json.load(sys.stdin))))')
  for pair in "$APP:$expected" "$APP/Contents/Helpers/jevision:"; do
    code=${pair%%:*}
    if [ "$(entitlements "$code")" != "${pair#*:}" ]; then
      echo "Unexpected entitlements on $code (expected: ${pair#*:}):" >&2
      codesign --display --entitlements - "$code" >&2 || true
      exit 1
    fi
  done
fi

# The debug symbols have to be the ones of the binaries that ship: crash reports are matched to
# them by UUID, one per architecture.
echo "==> Checking the debug symbols"
uuids() { dwarfdump --uuid "$1" | awk '{ print $2, $3 }' | sort; }
for pair in "MacOS/JEVision:JEVision.app" "Helpers/jevision:jevision"; do
  binary=$APP/Contents/${pair%%:*}
  symbols=build/dSYMs/${pair##*:}.dSYM/Contents/Resources/DWARF/$(basename "$binary")
  if [ -z "$(uuids "$symbols")" ] || [ "$(uuids "$binary")" != "$(uuids "$symbols")" ]; then
    echo "The symbols in ${symbols} don't belong to $binary." >&2
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
  xcrun stapler validate "$APP"
  spctl -a -vv -t execute "$APP"
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
  spctl -a -vv -t open --context context:primary-signature "$DMG"
elif [ -z "$ADHOC" ]; then
  echo "==> Gatekeeper (rejects the app until it is notarized):"
  spctl -a -vv -t execute "$APP" || true
  cat <<EOF

Signed, not notarized. To notarize, store an app-specific password once
(appleid.apple.com > Sign-In and Security > App-Specific Passwords):
  xcrun notarytool store-credentials jevision --apple-id you@example.com --team-id TEAMID
then run again with NOTARY_PROFILE=jevision.
EOF
fi

(cd build && shasum -a 256 "$(basename "$DMG")" > "$(basename "$DMG").sha256")

SYMBOLS=build/JEVision-$VERSION.dSYMs.zip
rm -f "$SYMBOLS"
ditto -c -k build/dSYMs "$SYMBOLS"

# The feed is signed after the disk image is final: any later change to it breaks the signature.
if [ -n "${SPARKLE_PUBLIC_ED_KEY:-}" ] && { [ -z "$ADHOC" ] || [ -n "${SPARKLE_ED_KEY_FILE:-}" ]; }; then
  echo "==> Signing the update feed"
  scripts/make-appcast.sh "$DMG"
fi
echo "==> $VERSION ($BUILD_NUMBER): $PWD/$DMG"
cat "$DMG.sha256"
