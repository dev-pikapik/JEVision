#!/bin/bash
# Takes the App Store screenshots of JEVision Remote in the built-in example, which needs no Mac
# and no network, on the two iPhone sizes App Store Connect takes, in English and Russian.
#
#   apps/ios/scripts/screenshots.sh [output folder]
#
# The pictures go to <output folder>/<language>/<size>/, build/ios-screenshots by default:
# en-US and ru as in apps/ios/fastlane/metadata, 6.9-inch (1320 × 2868) and 6.5-inch
# (1284 × 2778). Each runs on a new simulator, deleted afterwards, with the clock at 9:41 and a
# full battery, so no simulator of yours is changed. The number in a file's name is its place
# on the App Store page.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
OUT=${1:-$ROOT/build/ios-screenshots}
WORK=$(mktemp -d)
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
PROJECT=$ROOT/apps/ios/JEVisionRemote.xcodeproj
# The newest iOS runtime installed.
RUNTIME=$(xcrun simctl list runtimes -j | python3 -c '
import json, sys
runtimes = [r for r in json.load(sys.stdin)["runtimes"] if r["platform"] == "iOS" and r["isAvailable"]]
print(max(runtimes, key=lambda r: [int(n) for n in r["version"].split(".")])["identifier"] if runtimes else "")')
[ -n "$RUNTIME" ] || { echo "No iOS simulator runtime; install one in Xcode ▸ Settings ▸ Components." >&2; exit 1; }

UDID=
cleanup() {
  if [ -n "$UDID" ]; then
    xcrun simctl shutdown "$UDID" >/dev/null 2>&1 || true
    xcrun simctl delete "$UDID" >/dev/null 2>&1 || true
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT
trap 'exit 1' INT TERM

echo "==> Building for the simulator"
xcodebuild build-for-testing -quiet -project "$PROJECT" -scheme JEVisionRemote \
  -destination 'generic/platform=iOS Simulator' -derivedDataPath "$WORK/dd" \
  ARCHS=arm64 CODE_SIGN_IDENTITY=- CODE_SIGN_STYLE=Manual DEVELOPMENT_TEAM=

# size folder, simulator model, width, height
shoot() {
  local size=$1 model=$2 width=$3 height=$4 language folder name file found
  # Reports name the iPhone a run was started from, which here is the simulator.
  UDID=$(xcrun simctl create iPhone "com.apple.CoreSimulator.SimDeviceType.$model" "$RUNTIME")
  xcrun simctl boot "$UDID"
  xcrun simctl bootstatus "$UDID" -b >/dev/null
  xcrun simctl status_bar "$UDID" override --time 9:41 --batteryState charged --batteryLevel 100 \
    --dataNetwork wifi --wifiMode active --wifiBars 3 --cellularMode active --cellularBars 4 --operatorName ''
  xcrun simctl ui "$UDID" appearance light
  for language in en-US ru; do
    name=$size-$language
    echo "==> $size, $language"
    TEST_RUNNER_JEVISION_SCREENSHOTS_LANGUAGE=${language%%-*} \
      xcodebuild test-without-building -project "$PROJECT" -scheme JEVisionRemote \
      -destination "platform=iOS Simulator,id=$UDID" -derivedDataPath "$WORK/dd" \
      -only-testing:JEVisionRemoteUITests/ScreenshotTests -resultBundlePath "$WORK/$name.xcresult" \
      -collect-test-diagnostics never
    folder=$OUT/$language/$size
    rm -rf "$folder"
    mkdir -p "$folder" "$WORK/$name"
    xcrun xcresulttool export attachments --path "$WORK/$name.xcresult" --output-path "$WORK/$name" >/dev/null
    # The manifest maps exported files to the names the test gave them.
    python3 - "$WORK/$name" "$folder" <<'EOF'
import json, os, shutil, sys
src, out = sys.argv[1:]
for test in json.load(open(os.path.join(src, "manifest.json"))):
    for a in test["attachments"]:
        name = a["suggestedHumanReadableName"].split("_")[0]
        shutil.copy(os.path.join(src, a["exportedFileName"]), os.path.join(out, f"{name}.png"))
EOF
    for file in "$folder"/*.png; do
      found=$(sips -g pixelWidth -g pixelHeight "$file" | awk '/pixelWidth/ {w = $2} /pixelHeight/ {h = $2} END {print w " × " h}')
      [ "$found" = "$width × $height" ] || { echo "$file is $found, the App Store wants $width × $height" >&2; exit 1; }
    done
  done
  xcrun simctl shutdown "$UDID" >/dev/null 2>&1 || true
  xcrun simctl delete "$UDID" >/dev/null 2>&1 || true
  UDID=
}

shoot 6.9-inch iPhone-17-Pro-Max 1320 2868
shoot 6.5-inch iPhone-14-Plus 1284 2778
echo "Screenshots are in $OUT"
