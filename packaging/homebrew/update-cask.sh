#!/bin/bash
# Fills the version and checksum of jevision.rb from a release disk image made by
# apps/macos/scripts/release.sh. Publishing the cask to a tap is a separate, manual step.
#   update-cask.sh apps/macos/build/JEVision-1.2.0.dmg
set -euo pipefail

if [ $# -ne 1 ] || [ ! -f "$1" ]; then
  echo "Usage: $0 JEVision-<version>.dmg" >&2
  exit 2
fi
CASK=$(dirname "$0")/jevision.rb

# The version comes from the app inside, not from the file name.
MOUNT=$(mktemp -d)
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$MOUNT" "$1"
VERSION=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$MOUNT/JEVision.app/Contents/Info.plist")
hdiutil detach -quiet "$MOUNT"
rmdir "$MOUNT"
if [ "$(basename "$1")" != "JEVision-$VERSION.dmg" ]; then
  echo "The cask downloads JEVision-$VERSION.dmg, but the file is named $(basename "$1")." >&2
  exit 1
fi
SHA256=$(shasum -a 256 "$1" | cut -d ' ' -f 1)

sed -i '' -E \
  -e "s/^  version \"[^\"]*\"/  version \"$VERSION\"/" \
  -e "s/^  sha256 \"[0-9a-f]*\"/  sha256 \"$SHA256\"/" \
  "$CASK"
grep -q "version \"$VERSION\"" "$CASK" && grep -q "sha256 \"$SHA256\"" "$CASK"
ruby -c "$CASK" >/dev/null
echo "$CASK: $VERSION, $SHA256"
