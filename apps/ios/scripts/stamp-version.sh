#!/bin/bash
# Xcode build phase of the JEVisionRemote target: writes the version and build number from
# scripts/version.sh into the app's built Info.plist, so the iPhone app reports what the core and
# the Mac app report and the project file holds no version of its own.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
PLIST=$TARGET_BUILD_DIR/$INFOPLIST_PATH

stamp() { # key value; the keys are not in the processed plist of a fresh build, but are of a rebuild
  /usr/libexec/PlistBuddy -c "Delete :$1" "$PLIST" 2>/dev/null || true
  /usr/libexec/PlistBuddy -c "Add :$1 string $2" "$PLIST"
}
stamp CFBundleShortVersionString "$("$ROOT/scripts/version.sh" version)"
stamp CFBundleVersion "$("$ROOT/scripts/version.sh" build)"
