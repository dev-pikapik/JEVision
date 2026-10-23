#!/bin/bash
# The one place a release's version and build number come from.
#   scripts/version.sh          1.0.0            the version
#   scripts/version.sh build    128              the build number
#   scripts/version.sh full     1.0.0 (128)
# The version is `version` in the workspace manifest, so the core, the command line, the Mac app and
# the iPhone app all report the same one. The build number is BUILD_NUMBER when it is set, else the
# number of commits that lead to HEAD, which only ever grows along a branch; 1 outside a clone.
# A beta is a version such as 1.1.0-beta.1 (also -alpha.N and -rc.N): updates compare build numbers,
# so the beta updates to the release that follows it.
# JEVISION_VERSION stands in for the manifest's version in a test build, as the update test bench
# (apps/macos/scripts/update-e2e.sh) does to build a beta; scripts/check-version.sh refuses it.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)

VERSION=${JEVISION_VERSION:-$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml")}
if ! [[ $VERSION =~ ^[0-9]+\.[0-9]+\.[0-9]+(-(alpha|beta|rc)\.[1-9][0-9]*)?$ ]]; then
  echo "The version must be three numbers such as 1.0.0, or a beta such as 1.1.0-beta.1, got: '$VERSION'" >&2
  exit 1
fi
BUILD=${BUILD_NUMBER:-$(git -C "$ROOT" rev-list --count HEAD 2>/dev/null || echo 1)}
if ! [[ $BUILD =~ ^[1-9][0-9]*$ ]]; then
  echo "The build number must be a positive whole number, got: '$BUILD'" >&2
  exit 1
fi

case ${1:-version} in
  version) echo "$VERSION" ;;
  build) echo "$BUILD" ;;
  full) echo "$VERSION ($BUILD)" ;;
  *) echo "usage: scripts/version.sh [version|build|full]" >&2; exit 2 ;;
esac
