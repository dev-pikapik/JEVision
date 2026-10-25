#!/bin/bash
# The self-test: JEVision checks its own interface with its own tests (the selftest/ project).
#
#   scripts/selftest.sh [options] [test…]
#
#   test…         names of tests in selftest/, such as onboarding; default: all of them
#   --no-build    use the app already built in apps/macos/build instead of building it again
#   --keep        keep the working folder of every run (default: the last five runs)
#   -h, --help    show this text
#
#   SIGN_IDENTITY="Developer ID Application: Name (TEAMID)"   the certificate the build is signed
#                                          with; default: the first one in the Keychain
#   JEVISION_SELFTEST_QUIT_KEYS=cmd+q      override quit keys when a keyboard utility remaps them
#   JEVISION_SELFTEST_CLOSE_KEYS=cmd+w     override close-window keys for the same reason
#   JEVISION_SELFTEST_SCREEN_LOCK=folder   take this folder as a lock while the test uses the screen,
#                                          for people or tools that share one Mac (waits for a free lock)
#   JEVISION_SELFTEST_WORK=folder          the working folder of the run, new or empty; default:
#                                          build/selftest/<time>. With it the paths of both copies
#                                          are known before the run: folder/Driver/JEVision.app and
#                                          folder/Subject/JEVision.app.
#
# Two copies of the signed app apps/macos/build/JEVision.app take part, both in the working folder,
# so building again while a run goes on, or another run next to it, changes neither:
#   the driver   Driver/JEVision.app. It has the macOS permissions Screen Recording and Accessibility
#                (they are bound to the app's identifier and signature, not to where it is, so
#                Terminal and editors, which don't have them, hand the run to it with
#                `jevision run --through-app`).
#   the subject  Subject/JEVision.app: the one the tests operate. It has the same identifier and
#                signature, so macOS gives it the same permissions, and it runs with JEVISION_PROFILE,
#                so it never touches your own settings, projects or Keychain.
# Everything of a run is in the working folder: the project copy, the profiles of the subject, the
# reports (report/html, report/junit.xml) and the subject's logs. build/selftest/latest points to
# the last run in build/selftest.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$PWD
BUNDLE_ID=io.github.dev-pikapik.jevision

BUILD=1
KEEP=0
TESTS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --no-build) BUILD=0 ;;
    --keep) KEEP=1 ;;
    -h | --help) sed -n '2,/^set -e/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "Unknown option $1 (see --help)." >&2; exit 2 ;;
    *) TESTS+=("$1") ;;
  esac
  shift
done

if ! xcrun --find swift >/dev/null 2>&1; then
  export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
fi
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"

# The test moves the mouse and types: a locked screen takes that input instead.
screen_locked() {
  swift - <<'SWIFT'
import CoreGraphics
let d = CGSessionCopyCurrentDictionary() as? [String: Any]
print((d?["CGSSessionScreenIsLocked"] as? Bool) ?? false)
SWIFT
}

BUILT=apps/macos/build/JEVision.app
if [ "$BUILD" = 1 ]; then
  # A signature that is the same on every build keeps the macOS permissions: they belong to the
  # identifier and the signing team, and an ad-hoc signature has neither.
  if [ -z "${SIGN_IDENTITY:-}" ]; then
    SIGN_IDENTITY=$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application:.*\)"/\1/p' | head -n 1)
  fi
  if [ -z "$SIGN_IDENTITY" ]; then
    echo "No Developer ID Application certificate in the Keychain, and SIGN_IDENTITY is not set." >&2
    echo "The permissions Screen Recording and Accessibility belong to the app's signature, so the" >&2
    echo "self-test needs the same signature as the app you gave them to. List certificates with:" >&2
    echo "  security find-identity -v -p codesigning" >&2
    exit 2
  fi
  SIGN_IDENTITY="$SIGN_IDENTITY" PROFILE=${PROFILE:-debug} apps/macos/scripts/build.sh
fi
if [ ! -d "$BUILT" ]; then
  echo "$BUILT doesn't exist: run without --no-build." >&2
  exit 2
fi

if [ -n "${JEVISION_SELFTEST_WORK:-}" ]; then
  mkdir -p "$JEVISION_SELFTEST_WORK"
  WORK=$(cd "$JEVISION_SELFTEST_WORK" && pwd)
  if [ -n "$(ls -A "$WORK")" ]; then
    echo "$WORK isn't empty: JEVISION_SELFTEST_WORK has to name a new or empty folder." >&2
    exit 2
  fi
else
  WORK=$ROOT/build/selftest/$(date +%Y%m%d-%H%M%S)
fi
DRIVER=$WORK/Driver/JEVision.app
SUBJECT=$WORK/Subject/JEVision.app
PROJECT=$WORK/project
REPORT=$WORK/report
mkdir -p "$WORK/Driver" "$WORK/Subject" "$WORK/profiles" "$REPORT"
# Copies that share their disk space with the build (APFS clones), signature and all.
cp -Rc "$BUILT" "$DRIVER"
cp -Rc "$BUILT" "$SUBJECT"
codesign --verify --deep --strict "$DRIVER"
JEVISION=$DRIVER/Contents/Helpers/jevision

LOCK=${JEVISION_SELFTEST_SCREEN_LOCK:-}
HELD=0
cleanup() {
  # A failed run can leave the subject open; it lives only in the working folder.
  pkill -f "$SUBJECT/Contents/MacOS/JEVision" 2>/dev/null || true
  if [ "$HELD" = 1 ]; then rm -rf "$LOCK"; fi
  # With no earlier runs to prune, ls fails; that must not turn a passed run into a failed one.
  if [ "$KEEP" = 0 ]; then
    ls -1d "$ROOT"/build/selftest/2* 2>/dev/null | sort -r | tail -n +6 | while read -r old; do rm -rf "$old"; done || true
  fi
}
trap cleanup EXIT

# A free screen: wait for whoever uses it now, then keep it until the run ends.
if [ -n "$LOCK" ]; then
  SEEN=
  until mkdir "$LOCK" 2>/dev/null; do
    OWNER=$(cat "$LOCK/owner" 2>/dev/null || echo unknown)
    [ "$OWNER" = "$SEEN" ] || echo "The screen is in use ($OWNER); waiting…" >&2
    SEEN=$OWNER
    sleep 2
  done
  HELD=1
  git branch --show-current >"$LOCK/owner" 2>/dev/null || echo selftest >"$LOCK/owner"
fi
if [ "$(screen_locked)" != false ]; then
  echo "The screen is locked. Unlock it and run the self-test again: it clicks and types." >&2
  exit 2
fi

ditto selftest "$PROJECT"
# Only the disposable copy uses host-specific shortcuts; the source project stays portable.
if [ -n "${JEVISION_SELFTEST_QUIT_KEYS:-}${JEVISION_SELFTEST_CLOSE_KEYS:-}" ]; then
  swift - "$PROJECT" "${JEVISION_SELFTEST_QUIT_KEYS:-cmd+q}" "${JEVISION_SELFTEST_CLOSE_KEYS:-cmd+w}" <<'SWIFT'
import Foundation
let folder = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let shortcuts = ["cmd+q": CommandLine.arguments[2], "cmd+w": CommandLine.arguments[3]]
for file in try FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)
where file.lastPathComponent.hasSuffix(".flow.json") {
  var test = try JSONSerialization.jsonObject(with: Data(contentsOf: file)) as! [String: Any]
  guard var blocks = test["blocks"] as? [String: [String: Any]] else { continue }
  var changed = false
  for (id, var block) in blocks where block["type"] as? String == "pressKeys" {
    if let keys = block["keys"] as? String, let replacement = shortcuts[keys], replacement != keys {
      block["keys"] = replacement
      blocks[id] = block
      changed = true
    }
  }
  if changed {
    test["blocks"] = blocks
    try JSONSerialization.data(withJSONObject: test, options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes])
      .write(to: file, options: .atomic)
  }
}
SWIFT
fi
# Examples and their reference images are changed by the tests (accepting a new reference), so
# every run has its own copy.
ditto examples "$WORK/examples"
# The values the tests read; selftest/jevision.environments.json only names them.
cat >"$PROJECT/jevision.environments.json" <<JSON
{
  "format": 1,
  "default": "selftest",
  "environments": {
    "selftest": {
      "SELFTEST_WORK": "$WORK",
      "SELFTEST_PROFILES": "$WORK/profiles",
      "SELFTEST_EXAMPLES": "$WORK/examples",
      "SELFTEST_REPO": "$ROOT"
    }
  }
}
JSON
if [ -z "${JEVISION_SELFTEST_WORK:-}" ]; then ln -sfn "$WORK" "$ROOT/build/selftest/latest"; fi

RUN=()
if [ ${#TESTS[@]} -eq 0 ]; then
  RUN=("$PROJECT")
else
  for t in "${TESTS[@]}"; do RUN+=("$PROJECT/${t%.flow.json}.flow.json"); done
fi

echo "Self-test of $SUBJECT"
echo "Working folder $WORK"
START=$(date +%s)
set +e
JEVISION_PROFILE="$WORK/driver-profile" "$JEVISION" run "${RUN[@]}" \
  --through-app --app "$DRIVER" \
  --app-copy "$BUNDLE_ID=$SUBJECT" \
  --project "$PROJECT" --env selftest --lang en \
  --junit "$REPORT/junit.xml" --html "$REPORT/html" --video on-failure
STATUS=$?
set -e
END=$(date +%s)

echo
if [ "$STATUS" = 0 ]; then
  echo "Self-test passed in $((END - START)) s."
else
  echo "Self-test FAILED (exit $STATUS) after $((END - START)) s."
fi
echo "Reports:  $REPORT (open $REPORT/html)"
echo "Runs:     $PROJECT/.jevision/runs"
echo "Subject:  logs and settings in $WORK/profiles"
exit "$STATUS"
