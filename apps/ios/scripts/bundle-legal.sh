#!/bin/bash
# Xcode build phase of the JEVisionRemote target, after its resources: puts JEVision's license and
# the licenses of the code built into the app (scripts/licenses.py) in the app for About, and stops
# the build if the privacy manifest did not make it into the app.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
RESOURCES=$TARGET_BUILD_DIR/$UNLOCALIZED_RESOURCES_FOLDER_PATH

# Xcode starts scripts with a minimal PATH; the list of licenses comes from cargo.
PATH=$HOME/.cargo/bin:/opt/homebrew/opt/rustup/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin
if ! command -v cargo >/dev/null 2>&1; then
  echo "error: JEVision Remote needs Rust to list the licenses of its core. Install it from https://rustup.rs and build again." >&2
  exit 1
fi

cp "$ROOT/LICENSE" "$RESOURCES/LICENSE"
python3 "$ROOT/scripts/licenses.py" ios "$RESOURCES/ThirdPartyLicenses.md"

if ! plutil -lint -s "$RESOURCES/PrivacyInfo.xcprivacy"; then
  echo "error: the app has no valid PrivacyInfo.xcprivacy. It lives in apps/ios/JEVisionRemote and is copied with the app's resources." >&2
  exit 1
fi
