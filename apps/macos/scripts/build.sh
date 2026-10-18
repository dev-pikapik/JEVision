#!/bin/bash
# Builds build/JEVision.app without an Xcode project, with the jevision command-line tool in
# Contents/Helpers (not Contents/MacOS: on a case-insensitive disk "jevision" is "JEVision").
#   ARCHS="arm64"          build for one architecture (default: arm64 x86_64)
#   SIGN_IDENTITY="Name"   sign with a certificate (default: ad-hoc)
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
STAMP=$CORE/JEVisionCoreFFI.xcframework/Info.plist
if [ ! -f "$STAMP" ] || [ -n "$(find "$ROOT/crates" "$ROOT/Cargo.toml" "$ROOT/Cargo.lock" -newer "$STAMP" -print -quit)" ]; then
  "$ROOT/scripts/build-core-apple.sh"
fi

rm -rf build
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Helpers" "$APP/Contents/Resources" build/strings

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
  swiftc -O -whole-module-optimization -parse-as-library \
    -swift-version 6 -warnings-as-errors \
    -module-name JEVisionCore \
    -target "$ARCH-apple-macos$MIN_OS" \
    -I "$FFI/Headers" \
    -emit-module -emit-module-path "$MODULES/JEVisionCore.swiftmodule" \
    -emit-library -static -o "$MODULES/libJEVisionCore.a" \
    "$CORE/Sources/JEVisionCore/JEVisionCore.swift"
  swiftc -O -whole-module-optimization -parse-as-library \
    -swift-version 6 -warnings-as-errors \
    -module-name JEVisionHands \
    -target "$ARCH-apple-macos$MIN_OS" \
    -I "$MODULES" -I "$FFI/Headers" \
    -emit-module -emit-module-path "$MODULES/JEVisionHands.swiftmodule" \
    -emit-library -static -o "$MODULES/libJEVisionHands.a" \
    "${HANDS[@]}"
  swiftc -O -whole-module-optimization -parse-as-library \
    -swift-version 6 -warnings-as-errors \
    -module-name JEVision \
    -target "$ARCH-apple-macos$MIN_OS" \
    -I "$MODULES" -I "$FFI/Headers" \
    -L "$MODULES" -lJEVisionHands -lJEVisionCore -L "$FFI" -ljevision_ffi -framework SystemConfiguration \
    ${EMIT[@]+"${EMIT[@]}"} \
    "${SOURCES[@]}" \
    -o "build/JEVision-$ARCH"
  BINARIES+=("build/JEVision-$ARCH")
  EMIT=()
  swiftc -O -whole-module-optimization -parse-as-library \
    -swift-version 6 -warnings-as-errors \
    -module-name JEVisionCLI \
    -target "$ARCH-apple-macos$MIN_OS" \
    -I "$MODULES" -I "$FFI/Headers" \
    -L "$MODULES" -lJEVisionHands -lJEVisionCore -L "$FFI" -ljevision_ffi -framework SystemConfiguration \
    "${CLI[@]}" \
    -o "build/cli-$ARCH"
  TOOLS+=("build/cli-$ARCH")
done
lipo -create "${BINARIES[@]}" -output "$APP/Contents/MacOS/JEVision"
lipo -create "${TOOLS[@]}" -output "$APP/Contents/Helpers/jevision"
rm -r "${BINARIES[@]}" "${TOOLS[@]}" build/modules-*

python3 scripts/check-localization.py build/strings
rm -rf build/strings

cp Resources/Info.plist "$APP/Contents/"
cp Resources/AppIcon.icns "$APP/Contents/Resources/"
xcrun xcstringstool compile Resources/Localizable.xcstrings --output-directory "$APP/Contents/Resources" >/dev/null
xcrun xcstringstool compile Resources/InfoPlist.xcstrings --output-directory "$APP/Contents/Resources" >/dev/null

# Nested code first: signing the app seals what is inside it.
codesign --force --options runtime --identifier io.github.dev-pikapik.jevision.cli \
  --sign "${SIGN_IDENTITY:--}" "$APP/Contents/Helpers/jevision"
codesign --force --options runtime --sign "${SIGN_IDENTITY:--}" "$APP"
codesign --verify --strict "$APP"
echo "Built $PWD/$APP"
