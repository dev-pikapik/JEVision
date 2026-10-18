#!/bin/bash
# Builds the remote-control library for iPhone (device + Apple silicon simulator) and puts it
# where the iOS app picks it up:
#   apps/ios/Frameworks/JEVisionMobileFFI.xcframework   static library, C header, module map
#   apps/ios/Generated/JEVisionMobile.swift             generated Swift API
#   PROFILE=debug   build without optimizations (default: release)
set -euo pipefail
cd "$(dirname "$0")/.."

# xcodebuild -create-xcframework ships only with Xcode, not with the Command Line Tools.
if ! xcrun --find xcodebuild >/dev/null 2>&1 || ! xcodebuild -version >/dev/null 2>&1; then
  export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
fi

# Build for the oldest iOS the app runs on, C code in dependencies included.
export IPHONEOS_DEPLOYMENT_TARGET=26.0

PROFILE=${PROFILE:-release}
TARGETS=(aarch64-apple-ios aarch64-apple-ios-sim)
LIB=libjevision_mobile.a
FRAMEWORKS=apps/ios/Frameworks
GENERATED=apps/ios/Generated
XCFRAMEWORK=$FRAMEWORKS/JEVisionMobileFFI.xcframework
WORK=target/mobile-apple

case $PROFILE in
  release) CARGO_PROFILE=(--release) ;;
  debug) CARGO_PROFILE=() ;;
  *) echo "PROFILE must be release or debug, got: $PROFILE" >&2; exit 2 ;;
esac

rustup -q target add "${TARGETS[@]}"
for target in "${TARGETS[@]}"; do
  echo "==> Building remote control for $target"
  # Only the static library: with the Rust library built alongside, cargo skips link-time
  # optimization and the archive grows several times over.
  cargo rustc --locked -p jevision-mobile --lib --crate-type staticlib \
    ${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"} --target "$target"
done

rm -rf "$WORK" "$XCFRAMEWORK" "$GENERATED/JEVisionMobile.swift"
mkdir -p "$WORK/include" "$FRAMEWORKS" "$GENERATED"

echo "==> Generating Swift bindings"
cargo run --locked -q -p jevision-mobile --features bindgen --bin uniffi-bindgen-swift -- \
  "target/${TARGETS[0]}/$PROFILE/$LIB" "$WORK/include" \
  --headers --modulemap \
  --module-name JEVisionMobileFFI --modulemap-filename module.modulemap
cargo run --locked -q -p jevision-mobile --features bindgen --bin uniffi-bindgen-swift -- \
  "target/${TARGETS[0]}/$PROFILE/$LIB" "$GENERATED" --swift-sources

echo "==> Creating JEVisionMobileFFI.xcframework"
ARGS=()
for target in "${TARGETS[@]}"; do ARGS+=(-library "target/$target/$PROFILE/$LIB" -headers "$WORK/include"); done
xcodebuild -create-xcframework "${ARGS[@]}" -output "$XCFRAMEWORK" >/dev/null

rm -rf "$WORK"
echo "==> Done: $XCFRAMEWORK and $GENERATED/JEVisionMobile.swift"
