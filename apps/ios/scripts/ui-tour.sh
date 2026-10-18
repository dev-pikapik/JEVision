#!/bin/bash
# Walks JEVision Remote from pairing to a report in the iOS Simulator, against a stand-in Mac
# (`cargo run -p jevision-remote --example test-host`), in English and Russian, light and dark,
# and the largest text size, and saves a screenshot of every screen.
#
#   apps/ios/scripts/ui-tour.sh <simulator UDID> <output folder>
#
# The simulator is booted if needed and left as it was found: appearance, text size and
# Face ID enrollment are put back. Its keychain is reset, which forgets any paired Mac.
set -euo pipefail

UDID=${1:?usage: ui-tour.sh <simulator UDID> <output folder>}
OUT=${2:?usage: ui-tour.sh <simulator UDID> <output folder>}
ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
WORK=$(mktemp -d)
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
export PATH=$HOME/.cargo/bin:/opt/homebrew/opt/rustup/bin:$PATH

HOST_PID=
MATCHER_PID=
FAILED=
WAS_BOOTED=$(xcrun simctl list devices | grep -q "$UDID) (Booted)" && echo yes || echo no)
cleanup() {
  [ -n "$MATCHER_PID" ] && kill "$MATCHER_PID" 2>/dev/null || true
  [ -n "$HOST_PID" ] && kill "$HOST_PID" 2>/dev/null || true
  xcrun simctl ui "$UDID" appearance light >/dev/null 2>&1 || true
  xcrun simctl ui "$UDID" content_size large >/dev/null 2>&1 || true
  xcrun simctl spawn "$UDID" notifyutil -s com.apple.BiometricKit.enrollmentChanged 0 >/dev/null 2>&1 || true
  xcrun simctl spawn "$UDID" notifyutil -p com.apple.BiometricKit.enrollmentChanged >/dev/null 2>&1 || true
  [ "$WAS_BOOTED" = no ] && xcrun simctl shutdown "$UDID" >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT
trap 'exit 1' INT TERM

xcrun simctl boot "$UDID" 2>/dev/null || true
xcrun simctl bootstatus "$UDID" -b >/dev/null
xcrun simctl status_bar "$UDID" override --time 9:41 --batteryState charged --batteryLevel 100
# Pairing needs Face ID; enroll it and keep confirming every prompt while the tour runs.
xcrun simctl spawn "$UDID" notifyutil -s com.apple.BiometricKit.enrollmentChanged 1
xcrun simctl spawn "$UDID" notifyutil -p com.apple.BiometricKit.enrollmentChanged
while true; do
  xcrun simctl spawn "$UDID" notifyutil -p com.apple.BiometricKit_Sim.pearl.match >/dev/null 2>&1
  sleep 1
done &
MATCHER_PID=$!

cargo build -q --manifest-path "$ROOT/Cargo.toml" -p jevision-remote --example test-host
xcodebuild build-for-testing -quiet -project "$ROOT/apps/ios/JEVisionRemote.xcodeproj" \
  -scheme JEVisionRemote -destination "platform=iOS Simulator,id=$UDID" -derivedDataPath "$WORK/dd" \
  CODE_SIGN_IDENTITY=- CODE_SIGN_STYLE=Manual DEVELOPMENT_TEAM=

# name, language, appearance, text size
tour() {
  local name=$1 language=$2 appearance=$3 size=$4
  [ -n "$HOST_PID" ] && kill "$HOST_PID" 2>/dev/null || true
  rm -f "$WORK/link"
  "$ROOT/target/debug/examples/test-host" --step-ms 300 --link-file "$WORK/link" >/dev/null 2>&1 &
  HOST_PID=$!
  for _ in $(seq 50); do [ -s "$WORK/link" ] && break; sleep 0.2; done
  xcrun simctl keychain "$UDID" reset
  xcrun simctl uninstall "$UDID" io.github.dev-pikapik.jevision.remote 2>/dev/null || true
  xcrun simctl ui "$UDID" appearance "$appearance"
  xcrun simctl ui "$UDID" content_size "$size"
  TEST_RUNNER_JEVISION_UI_OFFER=$(cat "$WORK/link") TEST_RUNNER_JEVISION_UI_LANGUAGE=$language \
    xcodebuild test-without-building -quiet -project "$ROOT/apps/ios/JEVisionRemote.xcodeproj" \
    -scheme JEVisionRemote -destination "platform=iOS Simulator,id=$UDID" -derivedDataPath "$WORK/dd" \
    -only-testing:JEVisionRemoteUITests -resultBundlePath "$WORK/$name.xcresult" \
    -collect-test-diagnostics never || FAILED="$FAILED $name"
  mkdir -p "$WORK/$name"
  xcrun xcresulttool export attachments --path "$WORK/$name.xcresult" --output-path "$WORK/$name" >/dev/null
  # The manifest maps exported files to the names the test gave them.
  python3 - "$WORK/$name" "$OUT" "$name" <<'EOF'
import json, os, shutil, sys
src, out, tour = sys.argv[1:]
for test in json.load(open(os.path.join(src, "manifest.json"))):
    for a in test["attachments"]:
        name = a["suggestedHumanReadableName"].split("_")[0]
        shutil.copy(os.path.join(src, a["exportedFileName"]), os.path.join(out, f"ios-{tour}-{name}.png"))
EOF
}

tour en-light en light large
tour ru-dark ru dark large
tour ru-largest-text ru light accessibility-extra-extra-extra-large
echo "Screenshots are in $OUT"
[ -z "$FAILED" ] || { echo "Failed:$FAILED" >&2; exit 1; }
