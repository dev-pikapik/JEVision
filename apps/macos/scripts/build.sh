#!/bin/bash
# Builds build/JEVision.app without an Xcode project, with the jevision command-line tool in
# Contents/Helpers (not Contents/MacOS: on a case-insensitive disk "jevision" is "JEVision").
#   ARCHS="arm64"          build for one architecture (default: arm64 x86_64)
#   SIGN_IDENTITY="Name"   sign with a certificate (default: ad-hoc)
#   BUILD_NUMBER=42        CFBundleVersion (default: the number of commits, see scripts/version.sh)
#   SPARKLE_PUBLIC_ED_KEY  public key of update feeds and archives; without it the app never looks for updates
#   JEVISION_VERSION       another version, for the update test bench only (see scripts/version.sh)
# The version is the workspace version in the root Cargo.toml, shared with the core. Debug symbols
# of the app and the tool end up in build/dSYMs, not in the shipped binaries.
set -euo pipefail
cd "$(dirname "$0")/.."

# xcstringstool ships only with Xcode, not with the Command Line Tools.
if ! xcrun --find xcstringstool >/dev/null 2>&1; then
  export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
fi

APP=build/JEVision.app
MIN_OS=26.0
ARCHS=${ARCHS:-arm64 x86_64}

# The Rust core packaged for Swift, rebuilt whenever its sources are newer than the last build.
ROOT=../..
CORE=$ROOT/build/apple/JEVisionCore
FFI=$CORE/JEVisionCoreFFI.xcframework/macos-arm64_x86_64
STAMP=$CORE/.build-stamp
if [ ! -f "$STAMP" ] || [ "$(cat "$CORE/.build-profile" 2>/dev/null || true)" != "${PROFILE:-release}" ] || [ -n "$(find "$ROOT/crates" "$ROOT/Cargo.toml" "$ROOT/Cargo.lock" "$ROOT/apps/macos/Resources/Localizable.xcstrings" -newer "$STAMP" -print -quit)" ]; then
  "$ROOT/scripts/build-core-apple.sh"
fi

# Sparkle, at the version Package.swift pins, from the binary package SwiftPM downloads.
swift package resolve >/dev/null
SPARKLE=.build/artifacts/sparkle/Sparkle/Sparkle.xcframework/macos-arm64_x86_64

VERSION=$("$ROOT/scripts/version.sh" version)
BUILD_NUMBER=$("$ROOT/scripts/version.sh" build)

rm -rf build
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Helpers" "$APP/Contents/Frameworks" "$APP/Contents/Resources" build/strings

SOURCES=()
while IFS= read -r -d '' f; do SOURCES+=("$f"); done < <(find Sources/JEVision -name '*.swift' -print0)
HANDS=()
while IFS= read -r -d '' f; do HANDS+=("$f"); done < <(find Sources/JEVisionHands -name '*.swift' -print0)
CLI=()
while IFS= read -r -d '' f; do CLI+=("$f"); done < <(find Sources/JEVisionCLI -name '*.swift' -print0)

BINARIES=()
TOOLS=()
EMIT=(-Xfrontend -emit-localized-strings -Xfrontend -emit-localized-strings-path -Xfrontend build/strings)
for ARCH in $ARCHS; do
  echo "Building $ARCH"
  MODULES=build/modules-$ARCH
  mkdir -p "$MODULES"
  swiftc -O -g -whole-module-optimization -parse-as-library \
    -swift-version 6 -warnings-as-errors \
    -module-name JEVisionCore \
    -target "$ARCH-apple-macos$MIN_OS" \
    -I "$FFI/Headers" \
    -emit-module -emit-module-path "$MODULES/JEVisionCore.swiftmodule" \
    -emit-library -static -o "$MODULES/libJEVisionCore.a" \
    "$CORE/Sources/JEVisionCore/JEVisionCore.swift"
  swiftc -O -g -whole-module-optimization -parse-as-library \
    -swift-version 6 -warnings-as-errors \
    -module-name JEVisionHands \
    -target "$ARCH-apple-macos$MIN_OS" \
    -I "$MODULES" -I "$FFI/Headers" \
    -emit-module -emit-module-path "$MODULES/JEVisionHands.swiftmodule" \
    -emit-library -static -o "$MODULES/libJEVisionHands.a" \
    "${HANDS[@]}"
  # With -g, swiftc writes a JEVision-1.swiftmodule into the current folder unless told where the module goes.
  swiftc -O -g -whole-module-optimization -parse-as-library \
    -swift-version 6 -warnings-as-errors \
    -module-name JEVision \
    -target "$ARCH-apple-macos$MIN_OS" \
    -I "$MODULES" -I "$FFI/Headers" \
    -L "$MODULES" -lJEVisionHands -lJEVisionCore -L "$FFI" -ljevision_ffi -framework SystemConfiguration \
    -F "$SPARKLE" -framework Sparkle -Xlinker -rpath -Xlinker @executable_path/../Frameworks \
    -emit-executable -emit-module-path "$MODULES/JEVision.swiftmodule" \
    ${EMIT[@]+"${EMIT[@]}"} \
    "${SOURCES[@]}" \
    -o "build/JEVision-$ARCH"
  BINARIES+=("build/JEVision-$ARCH")
  EMIT=()
  swiftc -O -g -whole-module-optimization -parse-as-library \
    -swift-version 6 -warnings-as-errors \
    -module-name JEVisionCLI \
    -target "$ARCH-apple-macos$MIN_OS" \
    -I "$MODULES" -I "$FFI/Headers" \
    -L "$MODULES" -lJEVisionHands -lJEVisionCore -L "$FFI" -ljevision_ffi -framework SystemConfiguration \
    -emit-executable -emit-module-path "$MODULES/JEVisionCLI.swiftmodule" \
    "${CLI[@]}" \
    -o "build/cli-$ARCH"
  TOOLS+=("build/cli-$ARCH")
done
lipo -create "${BINARIES[@]}" -output "$APP/Contents/MacOS/JEVision"
lipo -create "${TOOLS[@]}" -output "$APP/Contents/Helpers/jevision"

# swiftc wrote a dSYM next to every per-architecture binary. Each product gets one dSYM with all
# architectures in build/dSYMs (release.sh archives them to symbolicate crash reports), and the
# shipped binaries lose their debug map, which only names temporary files. The dSYM matches its
# binary by UUID, which stripping and signing keep.
merge_dsym() { # thin binary prefix, dSYM name, binary name
  local dwarf=build/dSYMs/$2.dSYM/Contents/Resources/DWARF slices=() first=
  for ARCH in $ARCHS; do
    slices+=("build/$1-$ARCH.dSYM/Contents/Resources/DWARF/$1-$ARCH")
    first=${first:-build/$1-$ARCH.dSYM/Contents/Info.plist}
  done
  mkdir -p "$dwarf"
  lipo -create "${slices[@]}" -output "$dwarf/$3"
  cp "$first" "build/dSYMs/$2.dSYM/Contents/Info.plist"
  /usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier com.apple.xcode.dsym.$2" "build/dSYMs/$2.dSYM/Contents/Info.plist"
}
merge_dsym JEVision JEVision.app JEVision
merge_dsym cli jevision jevision
strip -S "$APP/Contents/MacOS/JEVision" "$APP/Contents/Helpers/jevision"
rm -r "${BINARIES[@]}" "${TOOLS[@]}" build/*.dSYM build/modules-*

python3 scripts/check-localization.py build/strings
rm -rf build/strings

cp Resources/Info.plist "$APP/Contents/"
PLIST="$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :CFBundleShortVersionString string $VERSION" -c "Add :CFBundleVersion string $BUILD_NUMBER" "$PLIST"
if [ -n "${SPARKLE_PUBLIC_ED_KEY:-}" ]; then
  if ! [[ $SPARKLE_PUBLIC_ED_KEY =~ ^[A-Za-z0-9+/]{43}=$ ]]; then
    echo "SPARKLE_PUBLIC_ED_KEY isn't an EdDSA public key (44 characters of base64): $SPARKLE_PUBLIC_ED_KEY" >&2
    exit 2
  fi
  # Sparkle checks the EdDSA signature of an update before it opens the archive, not after, and
  # accepts only an update feed signed with the same key.
  /usr/libexec/PlistBuddy -c "Add :SUPublicEDKey string $SPARKLE_PUBLIC_ED_KEY" \
    -c "Add :SUVerifyUpdateBeforeExtraction bool true" -c "Add :SURequireSignedFeed bool true" "$PLIST"
fi
ditto "$SPARKLE/Sparkle.framework" "$APP/Contents/Frameworks/Sparkle.framework"
cp Resources/AppIcon.icns "$APP/Contents/Resources/"
# About shows JEVision's license and those of the code built into it, from the app itself.
cp "$ROOT/LICENSE" "$APP/Contents/Resources/"
python3 "$ROOT/scripts/licenses.py" macos "$APP/Contents/Resources/ThirdPartyLicenses.md"
# The privacy manifest: the app declares no tracking or collected data, and why it uses the APIs
# Apple asks a reason for. The check reads the copy in the app, so a missing or broken one stops the build.
cp Resources/PrivacyInfo.xcprivacy "$APP/Contents/Resources/"
plutil -lint -s "$APP/Contents/Resources/PrivacyInfo.xcprivacy"
# The guides open from disk, so they work offline and match this version; the example project is
# copied out of here, never run in place.
ditto "$ROOT/docs" "$APP/Contents/Resources/docs"
rm "$APP/Contents/Resources/docs/check-links.py" "$APP/Contents/Resources/docs/build-search-index.py"
rm -r "$APP/Contents/Resources/docs/maintainers"
ditto "$ROOT/examples" "$APP/Contents/Resources/examples"
xcrun xcstringstool compile Resources/Localizable.xcstrings --output-directory "$APP/Contents/Resources" >/dev/null
xcrun xcstringstool compile Resources/InfoPlist.xcstrings --output-directory "$APP/Contents/Resources" >/dev/null

# Nested code first, innermost out: signing a bundle seals what is inside it. Everything runs
# with the hardened runtime; only the app itself has an entitlement (Sparkle's downloader keeps
# its own).
SIGN=(codesign --force --options runtime --sign "${SIGN_IDENTITY:--}")
if [ -n "${SIGN_IDENTITY:-}" ]; then SIGN+=(--timestamp); fi
SPARKLE_CODE="$APP/Contents/Frameworks/Sparkle.framework/Versions/B"
"${SIGN[@]}" "$SPARKLE_CODE/XPCServices/Installer.xpc"
"${SIGN[@]}" --preserve-metadata=entitlements "$SPARKLE_CODE/XPCServices/Downloader.xpc"
"${SIGN[@]}" "$SPARKLE_CODE/Autoupdate"
"${SIGN[@]}" "$SPARKLE_CODE/Updater.app"
"${SIGN[@]}" "$APP/Contents/Frameworks/Sparkle.framework"
"${SIGN[@]}" --identifier io.github.dev-pikapik.jevision.cli "$APP/Contents/Helpers/jevision"
# The app's one entitlement: AppleScript blocks send Apple events to other apps, which the hardened
# runtime blocks without it (the system still asks the user once per target app).
cp Resources/JEVision.entitlements build/app.entitlements
if [ -z "${SIGN_IDENTITY:-}" ]; then
  # Library validation wants the app and Sparkle from one team, and ad-hoc code has none.
  /usr/libexec/PlistBuddy -c "Add :com.apple.security.cs.disable-library-validation bool true" build/app.entitlements >/dev/null
fi
"${SIGN[@]}" --entitlements build/app.entitlements "$APP"
rm build/app.entitlements
codesign --verify --deep --strict "$APP"
echo "Built $PWD/$APP"
