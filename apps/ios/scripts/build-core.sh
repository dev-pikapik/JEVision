#!/bin/bash
# Xcode build phase of the JEVisionMobile target: runs scripts/build-mobile-apple.sh when the
# remote-control library or its Swift API is missing or older than the Rust code it is built
# from, so a fresh clone builds with a single xcodebuild command.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
STAMP=$ROOT/apps/ios/Frameworks/JEVisionMobileFFI.xcframework/Info.plist
SWIFT=$ROOT/apps/ios/Generated/JEVisionMobile.swift
# Debug app builds use the debug core; an archive always rebuilds a cached debug core for release.
case ${CONFIGURATION:-Release} in
  Debug) PROFILE=${PROFILE:-debug} ;;
  *) PROFILE=${PROFILE:-release} ;;
esac

# Xcode starts scripts with a minimal PATH and with its own SDK settings, which would point
# cargo's host build scripts at the iPhone SDK. Build in a clean environment instead.
PATH=$HOME/.cargo/bin:/opt/homebrew/opt/rustup/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin
if ! command -v cargo >/dev/null 2>&1; then
  echo "error: JEVision Remote needs Rust to build its core. Install it from https://rustup.rs and build again." >&2
  exit 1
fi

if [ -f "$STAMP" ] && [ -f "$SWIFT" ] && [ "$(cat "$ROOT/apps/ios/Frameworks/.build-profile" 2>/dev/null || true)" = "$PROFILE" ]; then
  # Every local crate the library is made of, plus the workspace manifest and lock file.
  SOURCES=("$ROOT/Cargo.toml" "$ROOT/Cargo.lock" "$ROOT/apps/macos/Resources/Localizable.xcstrings")
  while IFS= read -r dir; do SOURCES+=("$dir"); done < <(
    cd "$ROOT" && cargo tree --locked -q -p jevision-mobile -e normal,build --prefix none \
      | sed -n 's/^[^ ]* v[^ ]* (\(\/[^)]*\)).*/\1/p' | sort -u)
  if [ -z "$(find "${SOURCES[@]}" -newer "$STAMP" -print -quit)" ]; then
    exit 0
  fi
fi

exec env -i HOME="$HOME" USER="${USER:-}" PATH="$PATH" TMPDIR="${TMPDIR:-/tmp}" \
  ${RUSTUP_HOME:+RUSTUP_HOME="$RUSTUP_HOME"} ${CARGO_HOME:+CARGO_HOME="$CARGO_HOME"} \
  PROFILE="$PROFILE" \
  DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}" \
  "$ROOT/scripts/build-mobile-apple.sh"
