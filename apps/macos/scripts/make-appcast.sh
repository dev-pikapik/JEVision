#!/bin/bash
# Makes the Sparkle appcast for a release disk image made by release.sh. Doesn't publish anything.
#   make-appcast.sh build/JEVision-1.2.0.dmg [current-appcast.xml]
# Starts from the given appcast, or else from the published one, so earlier versions stay listed,
# and signs the new entry with the update key in the Keychain (see "Releasing" in README.md).
# Writes build/appcast/appcast.xml: publish it at the SUFeedURL of Resources/Info.plist, and
# the disk image as a GitHub release asset under the tag v<version>.
#   SPARKLE_KEY_ACCOUNT   Keychain account of the update key (default: jevision)
#   RELEASE_NOTES=file    release notes, .html or .md, to embed in the new entry
set -euo pipefail

if [ $# -lt 1 ] || [ $# -gt 2 ] || [ ! -f "$1" ]; then
  echo "Usage: $0 JEVision-<version>.dmg [current-appcast.xml]" >&2
  exit 2
fi
DMG=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
CURRENT=${2:+$(cd "$(dirname "$2")" && pwd)/$(basename "$2")}
cd "$(dirname "$0")/.."

REPO=https://github.com/dev-pikapik/JEVision
FEED=$(/usr/libexec/PlistBuddy -c "Print :SUFeedURL" Resources/Info.plist)

# The version comes from the app inside, not from the file name.
MOUNT=$(mktemp -d)
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$MOUNT" "$DMG"
VERSION=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$MOUNT/JEVision.app/Contents/Info.plist")
KEY=$(plutil -extract SUPublicEDKey raw -o - "$MOUNT/JEVision.app/Contents/Info.plist" 2>/dev/null || true)
hdiutil detach -quiet "$MOUNT"
rmdir "$MOUNT"
if [ -z "$KEY" ]; then
  echo "$1 was built without an update key, so it couldn't install updates: build it again with the key." >&2
  exit 1
fi

swift package resolve >/dev/null
SPARKLE_BIN=.build/artifacts/sparkle/Sparkle/bin
ACCOUNT=${SPARKLE_KEY_ACCOUNT:-jevision}
# generate_appcast only warns without the private key, and its appcast would be useless.
if ! public=$("$SPARKLE_BIN/generate_keys" --account "$ACCOUNT" -p 2>/dev/null) || [ "$public" != "$KEY" ]; then
  echo "The Keychain has no update key for account $ACCOUNT matching the one in $1." >&2
  exit 1
fi
OUT=build/appcast
rm -rf "$OUT"
mkdir -p "$OUT"
if [ -n "$CURRENT" ]; then
  cp "$CURRENT" "$OUT/appcast.xml"
elif curl -fsL -o "$OUT/appcast.xml" "$FEED"; then
  echo "==> Starting from $FEED"
else
  rm -f "$OUT/appcast.xml"
  echo "==> No appcast at $FEED yet: starting a new one"
fi
cp "$DMG" "$OUT/"
if [ -n "${RELEASE_NOTES:-}" ]; then
  cp "$RELEASE_NOTES" "$OUT/$(basename "${DMG%.dmg}").${RELEASE_NOTES##*.}"
fi

"$SPARKLE_BIN/generate_appcast" \
  --account "$ACCOUNT" \
  --download-url-prefix "$REPO/releases/download/v$VERSION/" \
  --link "$REPO" \
  --embed-release-notes \
  -o "$OUT/appcast.xml" "$OUT"

echo "==> $PWD/$OUT/appcast.xml (version $VERSION)"
