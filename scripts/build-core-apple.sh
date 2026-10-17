#!/bin/bash
# Builds the Rust core for macOS (arm64 + x86_64) and packages it for Swift:
#   build/apple/JEVisionCore/                          Swift package to depend on
#     JEVisionCoreFFI.xcframework                      static library, C header, module map
#     Sources/JEVisionCore/JEVisionCore.swift          generated Swift API
# Check the result with: swift test --package-path build/apple/JEVisionCore
#   PROFILE=debug   build without optimizations (default: release)
set -euo pipefail
cd "$(dirname "$0")/.."

# xcodebuild -create-xcframework ships only with Xcode, not with the Command Line Tools.
if ! xcrun --find xcodebuild >/dev/null 2>&1 || ! xcodebuild -version >/dev/null 2>&1; then
  export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
fi

PROFILE=${PROFILE:-release}
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
LIB=libjevision_ffi.a
OUT=build/apple
PACKAGE=$OUT/JEVisionCore
WORK=$OUT/work

case $PROFILE in
  release) CARGO_PROFILE=(--release) ;;
  debug) CARGO_PROFILE=() ;;
  *) echo "PROFILE must be release or debug, got: $PROFILE" >&2; exit 2 ;;
esac

rustup -q target add "${TARGETS[@]}"
for target in "${TARGETS[@]}"; do
  echo "==> Building core for $target"
  cargo build --locked -p jevision-ffi --lib "${CARGO_PROFILE[@]}" --target "$target"
done

rm -rf "$OUT"
mkdir -p "$WORK/include" "$PACKAGE/Sources/JEVisionCore"

echo "==> Generating Swift bindings"
cargo run --locked -q -p jevision-ffi --features bindgen --bin uniffi-bindgen-swift -- \
  "target/${TARGETS[0]}/$PROFILE/$LIB" "$WORK/include" \
  --headers --modulemap \
  --module-name JEVisionCoreFFI --modulemap-filename module.modulemap
cargo run --locked -q -p jevision-ffi --features bindgen --bin uniffi-bindgen-swift -- \
  "target/${TARGETS[0]}/$PROFILE/$LIB" "$PACKAGE/Sources/JEVisionCore" --swift-sources

echo "==> Creating JEVisionCoreFFI.xcframework"
LIBS=()
for target in "${TARGETS[@]}"; do LIBS+=("target/$target/$PROFILE/$LIB"); done
lipo -create "${LIBS[@]}" -output "$WORK/$LIB"
xcodebuild -create-xcframework \
  -library "$WORK/$LIB" -headers "$WORK/include" \
  -output "$PACKAGE/JEVisionCoreFFI.xcframework" >/dev/null

cp -R crates/jevision-ffi/swift/. "$PACKAGE/"
rm -rf "$WORK"
echo "==> Done: $PACKAGE"
