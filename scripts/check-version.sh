#!/bin/bash
# Checks that the whole project agrees on its version, for CI and before a release.
#   scripts/check-version.sh              the checkout is consistent
#   scripts/check-version.sh v1.0.0       ...and the tag is this version (a release build)
# The version lives in the workspace Cargo.toml only (scripts/version.sh); this fails when a second
# copy appears, when the lock file or the changelog lags behind, or when a tag disagrees.
set -euo pipefail
cd "$(dirname "$0")/.."

fail() { echo "check-version: $1" >&2; exit 1; }
[ -z "${JEVISION_VERSION:-}" ] || fail "JEVISION_VERSION is set: it is for test builds only, a release takes its version from Cargo.toml"
VERSION=$(scripts/version.sh version)

lock=$(awk '/^name = "jevision-core"$/ { getline; gsub(/version = |"/, ""); print; exit }' Cargo.lock)
[ "$lock" = "$VERSION" ] || fail "Cargo.lock has jevision-core $lock, Cargo.toml has $VERSION: run cargo update --workspace"

# The update window shows these notes in English or in Russian, after the language of the Mac.
for changelog in CHANGELOG.md CHANGELOG.ru.md; do
  grep -q "^## \[$VERSION\] - [0-9]\{4\}-[0-9]\{2\}-[0-9]\{2\}\$" "$changelog" \
    || fail "$changelog has no '## [$VERSION] - YYYY-MM-DD' section"
done

# The apps take their version from the build, so the checked-in files must not carry one.
if grep -q "MARKETING_VERSION\|CURRENT_PROJECT_VERSION" apps/ios/JEVisionRemote.xcodeproj/project.pbxproj; then
  fail "the iPhone project hard-codes a version: it comes from scripts/stamp-version.sh"
fi
grep -q "stamp-version.sh" apps/ios/JEVisionRemote.xcodeproj/project.pbxproj \
  || fail "the iPhone project no longer stamps the version"
for plist in apps/macos/Resources/Info.plist apps/ios/JEVisionRemote/Info.plist; do
  if grep -q "CFBundleShortVersionString\|CFBundleVersion" "$plist"; then
    fail "$plist hard-codes a version"
  fi
done

# Where the app, and `brew livecheck`, look for updates: the appcast of the latest published release.
FEED=https://github.com/dev-pikapik/JEVision/releases/latest/download/appcast.xml
grep -q "<string>$FEED</string>" apps/macos/Resources/Info.plist \
  || fail "SUFeedURL in apps/macos/Resources/Info.plist isn't $FEED"
grep -q "\"$FEED\"" packaging/homebrew/jevision.rb \
  || fail "the livecheck URL of packaging/homebrew/jevision.rb isn't $FEED"
# Betas: one feed that a fixed pre-release keeps, as `latest` never points at a pre-release.
BETA_FEED=https://github.com/dev-pikapik/JEVision/releases/download/appcast-beta/appcast-beta.xml
grep -q "<string>$BETA_FEED</string>" apps/macos/Resources/Info.plist \
  || fail "JEVisionBetaFeedURL in apps/macos/Resources/Info.plist isn't $BETA_FEED"

# The bundle identifiers are the app's name for macOS and the App Store: Accessibility and Screen
# Recording permissions, settings, Keychain items and the App Store listing hang on them, so a change
# is a different app. They never change.
grep -q "<string>io.github.dev-pikapik.jevision</string>" apps/macos/Resources/Info.plist \
  || fail "the bundle identifier of the Mac app isn't io.github.dev-pikapik.jevision"
grep -q -- "--identifier io.github.dev-pikapik.jevision.cli " apps/macos/scripts/build.sh \
  || fail "the identifier of the command-line tool isn't io.github.dev-pikapik.jevision.cli"
grep -q 'PRODUCT_BUNDLE_IDENTIFIER = "io.github.dev-pikapik.jevision.remote";' apps/ios/JEVisionRemote.xcodeproj/project.pbxproj \
  || fail "the bundle identifier of the iPhone app isn't io.github.dev-pikapik.jevision.remote"

if [ $# -gt 0 ]; then
  [ "$1" = "v$VERSION" ] || fail "the tag is $1, Cargo.toml says v$VERSION"
fi
echo "version $VERSION is consistent"
