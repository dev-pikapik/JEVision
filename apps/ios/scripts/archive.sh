#!/bin/bash
# Archives JEVision Remote for the App Store and checks what would be sent.
#
#   apps/ios/scripts/archive.sh            an unsigned archive, then a check of what is in it;
#                                          needs no Apple account
#   apps/ios/scripts/archive.sh validate   a signed archive, exported as an .ipa that App Store
#                                          Connect validates; nothing is uploaded
#   apps/ios/scripts/archive.sh upload     the same, then the .ipa is uploaded to App Store Connect
#
# validate and upload need:
#   DEVELOPMENT_TEAM             the Team ID, 10 characters
#   PROVISIONING_PROFILE         the name of the App Store provisioning profile for
#                                io.github.dev-pikapik.jevision.remote
#   APP_STORE_CONNECT_KEY_ID     the App Store Connect API key: its ID,
#   APP_STORE_CONNECT_ISSUER_ID  its issuer ID,
#   APP_STORE_CONNECT_KEY_FILE   and the path to its AuthKey_<ID>.p8 file
# and an Apple Distribution certificate with its private key in the keychain. Signing is manual:
# xcodebuild may download the named profile with the key, but never creates a profile, a
# certificate or an app ID.
#
# Everything goes to build/ios-archive (ARCHIVE_DIR overrides it): JEVisionRemote.xcarchive and,
# when signed, ExportOptions.plist and JEVisionRemote.ipa.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
MODE=${1:-check}
BUNDLE_ID=io.github.dev-pikapik.jevision.remote
OUT=${ARCHIVE_DIR:-$ROOT/build/ios-archive}
ARCHIVE=$OUT/JEVisionRemote.xcarchive
APP=$ARCHIVE/Products/Applications/JEVisionRemote.app
IPA=$OUT/JEVisionRemote.ipa

fail() {
  echo "error: $*" >&2
  exit 1
}

case $MODE in
  check | validate | upload) ;;
  *)
    echo "usage: $0 [check | validate | upload]" >&2
    exit 2
    ;;
esac

if [ "$MODE" != check ]; then
  missing=()
  for name in DEVELOPMENT_TEAM PROVISIONING_PROFILE APP_STORE_CONNECT_KEY_ID \
    APP_STORE_CONNECT_ISSUER_ID APP_STORE_CONNECT_KEY_FILE; do
    [ -n "${!name:-}" ] || missing+=("$name")
  done
  [ ${#missing[@]} -eq 0 ] || fail "$MODE needs ${missing[*]}; the top of $0 says what each is."
  [ -f "$APP_STORE_CONNECT_KEY_FILE" ] || fail "there is no key file at $APP_STORE_CONNECT_KEY_FILE"
  AUTH=(-authenticationKeyPath "$APP_STORE_CONNECT_KEY_FILE" -authenticationKeyID "$APP_STORE_CONNECT_KEY_ID"
    -authenticationKeyIssuerID "$APP_STORE_CONNECT_ISSUER_ID")
  ALTOOL_AUTH=(--api-key "$APP_STORE_CONNECT_KEY_ID" --api-issuer "$APP_STORE_CONNECT_ISSUER_ID"
    --p8-file-path "$APP_STORE_CONNECT_KEY_FILE")
fi

rm -rf "$OUT"
mkdir -p "$OUT"

echo "==> Archiving JEVision Remote ($MODE)"
ARCHIVE_ARGS=(-project "$ROOT/apps/ios/JEVisionRemote.xcodeproj" -scheme JEVisionRemote -configuration Release
  -destination generic/platform=iOS -archivePath "$ARCHIVE")
if [ "$MODE" = check ]; then
  xcodebuild archive "${ARCHIVE_ARGS[@]}" CODE_SIGNING_ALLOWED=NO
else
  # The profile goes to the app only: the core's static framework takes none, and naming one
  # for every target would fail on it.
  xcodebuild archive "${ARCHIVE_ARGS[@]}" -allowProvisioningUpdates "${AUTH[@]}" \
    DEVELOPMENT_TEAM="$DEVELOPMENT_TEAM" CODE_SIGN_STYLE=Manual CODE_SIGN_IDENTITY="Apple Distribution" \
    'PROVISIONING_PROFILE_SPECIFIER=$(JEVISION_PROFILE_$(WRAPPER_EXTENSION))' \
    JEVISION_PROFILE_app="$PROVISIONING_PROFILE"
fi

echo "==> Checking the archive"
[ -d "$APP" ] || fail "the archive has no app at $APP"
problems=()
plist=$APP/Info.plist
value() { /usr/libexec/PlistBuddy -c "Print :$1" "$plist" 2>/dev/null || true; }
expect() {
  local actual
  actual=$(value "$1")
  [ "$actual" = "$2" ] || problems+=("Info.plist $1 is '$actual', expected '$2'")
}

expect CFBundleIdentifier "$BUNDLE_ID"
expect CFBundleShortVersionString "$("$ROOT/scripts/version.sh")"
expect CFBundleVersion "$("$ROOT/scripts/version.sh" build)"
# The App Store takes up to three numbers here; a beta such as 1.1.0-beta.1 can't be sent.
[[ $(value CFBundleShortVersionString) =~ ^[0-9]+(\.[0-9]+){0,2}$ ]] ||
  problems+=("the version '$(value CFBundleShortVersionString)' is not one the App Store takes")
expect UIDeviceFamily:0 1
[ -z "$(value UIDeviceFamily:1)" ] || problems+=("the app is not for iPhone only (UIDeviceFamily)")
[ -n "$(value MinimumOSVersion)" ] || problems+=("Info.plist has no MinimumOSVersion")
case $(value ITSAppUsesNonExemptEncryption) in
  true | false) ;;
  *) problems+=("Info.plist has no ITSAppUsesNonExemptEncryption, so every upload asks about encryption") ;;
esac
for key in NSCameraUsageDescription NSFaceIDUsageDescription NSLocalNetworkUsageDescription; do
  [ -n "$(value "$key")" ] || problems+=("Info.plist has no $key")
done
expect CFBundleIcons:CFBundlePrimaryIcon:CFBundleIconName AppIcon
[ -f "$APP/Assets.car" ] || problems+=("the app has no asset catalog")
assets=$(xcrun assetutil --info "$APP/Assets.car" 2>/dev/null || true)
grep -q '"PixelWidth" : 1024' <<<"$assets" || problems+=("the asset catalog has no 1024-pixel app icon")
if [ ! -f "$APP/PrivacyInfo.xcprivacy" ]; then
  problems+=("the app has no privacy manifest PrivacyInfo.xcprivacy")
elif ! plutil -lint -s "$APP/PrivacyInfo.xcprivacy"; then
  problems+=("the privacy manifest is not a valid property list")
fi
for language in en ru; do
  [ -d "$APP/$language.lproj" ] || problems+=("the app has no $language.lproj")
done
archs=$(lipo -archs "$APP/JEVisionRemote")
[ "$archs" = arm64 ] || problems+=("the app is built for '$archs', expected arm64")
load=$(otool -l "$APP/JEVisionRemote")
grep -A3 LC_BUILD_VERSION <<<"$load" | grep -q 'platform 2$' || problems+=("the app is not built for iOS devices")
[ -d "$ARCHIVE/dSYMs/JEVisionRemote.app.dSYM" ] || problems+=("the archive has no debug symbols")

if [ "$MODE" != check ]; then
  codesign --verify --deep --strict "$APP" 2>&1 || problems+=("the app's signature does not verify")
  [ -f "$APP/embedded.mobileprovision" ] || problems+=("the app has no provisioning profile")
  entitlements=$(codesign -d --entitlements - --xml "$APP" 2>/dev/null || true)
  [ "$(plutil -extract get-task-allow raw - <<<"$entitlements" 2>/dev/null || true)" != true ] ||
    problems+=("the app is signed for debugging (get-task-allow)")
fi

if [ ${#problems[@]} -gt 0 ]; then
  printf 'error: %s\n' "${problems[@]}" >&2
  exit 1
fi
echo "The archive is fine: version $(value CFBundleShortVersionString) ($(value CFBundleVersion)), \
iOS $(value MinimumOSVersion), $archs, encryption $(value ITSAppUsesNonExemptEncryption)."

if [ "$MODE" = check ]; then
  echo "Not signed: run '$0 validate' with the account's key to have App Store Connect check it."
  exit 0
fi

echo "==> Exporting for the App Store"
python3 - "$OUT/ExportOptions.plist" "$DEVELOPMENT_TEAM" "$BUNDLE_ID" "$PROVISIONING_PROFILE" <<'EOF'
import plistlib, sys
path, team, bundle, profile = sys.argv[1:]
with open(path, "wb") as file:
    plistlib.dump({
        "method": "app-store-connect",
        "destination": "export",
        "teamID": team,
        "signingStyle": "manual",
        "signingCertificate": "Apple Distribution",
        "provisioningProfiles": {bundle: profile},
        "uploadSymbols": True,
        "manageAppVersionAndBuildNumber": False,
    }, file)
EOF
xcodebuild -exportArchive -archivePath "$ARCHIVE" -exportPath "$OUT" \
  -exportOptionsPlist "$OUT/ExportOptions.plist" -allowProvisioningUpdates "${AUTH[@]}"
[ -f "$IPA" ] || fail "the export made no $IPA"

echo "==> App Store Connect validates the build"
xcrun altool --validate-app "$IPA" -t ios "${ALTOOL_AUTH[@]}"

if [ "$MODE" = upload ]; then
  echo "==> Uploading to App Store Connect"
  xcrun altool --upload-package "$IPA" -t ios "${ALTOOL_AUTH[@]}"
  echo "Uploaded. The build appears in App Store Connect ▸ TestFlight once Apple has processed it."
else
  echo "Valid: $IPA. '$0 upload' sends it."
fi
