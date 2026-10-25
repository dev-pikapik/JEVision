#!/bin/bash
# The update test bench: builds JEVision twice, as a beta (<version>-beta.1, build 2) and as the
# release after it (<version>, build 3), makes their signed update feeds with make-appcast.sh, serves
# them on this Mac and checks with update-probe.swift what an installed copy is offered: the update,
# its notes in English and in Russian, its data format, no older version, betas only on the beta
# channel, and no feed that is changed, unsigned or signed with another key. Doesn't open the app:
# it prints the commands to try the update window by hand. See "Testing updates locally" in
# docs/maintainers/releasing.md.
#   SPARKLE_ED_KEY_FILE=bench.key SPARKLE_PUBLIC_ED_KEY=… apps/macos/scripts/update-e2e.sh
#   SPARKLE_ED_KEY_FILE    private key of a throwaway key pair made for the bench, never the release key
#   SPARKLE_PUBLIC_ED_KEY  its public key
#   PORT=8765              port of the web server, on localhost only
#   ARCHS                  as for build.sh (default: the architecture of this Mac)
# The beta is built without touching Cargo.toml: JEVISION_VERSION gives the version to build.sh
# (scripts/version.sh). Writes build/update-e2e/ at the top of the repository, as every build.sh
# uses a separate output folder inside apps/macos/build.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$(cd ../.. && pwd)
OUT=$ROOT/build/update-e2e
PORT=${PORT:-8765}
export ARCHS=${ARCHS:-$(uname -m)}

if [ -z "${SPARKLE_ED_KEY_FILE:-}" ] || [ -z "${SPARKLE_PUBLIC_ED_KEY:-}" ]; then
  echo "Set SPARKLE_ED_KEY_FILE and SPARKLE_PUBLIC_ED_KEY to a throwaway key pair, see docs/maintainers/releasing.md." >&2
  exit 2
fi
export SPARKLE_ED_KEY_FILE=$(cd "$(dirname "$SPARKLE_ED_KEY_FILE")" && pwd)/$(basename "$SPARKLE_ED_KEY_FILE")
export SPARKLE_PUBLIC_ED_KEY
CARGO_VERSION=$("$ROOT/scripts/version.sh" version)
RELEASE=${CARGO_VERSION%%-*}
BETA=$RELEASE-beta.1
URL=http://localhost:$PORT
BUILD_OUTPUT=build/update-e2e-stage
SERVER_PID=
SERVER_NONCE_FILE=
cleanup() {
  if [[ -n $SERVER_PID ]] && jobs -pr | grep -qx "$SERVER_PID"; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  if [[ -n $SERVER_NONCE_FILE ]]; then rm -f "$SERVER_NONCE_FILE"; fi
  rm -f "$OUT/server.pid"
  rm -rf "$BUILD_OUTPUT" "$OUT/stage"
}
trap cleanup EXIT

# A saved PID may have been reused by an unrelated program since the previous run.
python3 - "$OUT/server.pid" "$OUT/www" <<'PYEND'
import os,re,signal,subprocess,sys
from pathlib import Path
pid_file,directory=Path(sys.argv[1]),sys.argv[2]
try:
    pid=int(pid_file.read_text().strip())
    if 1 < pid <= 2_147_483_647:
        command=subprocess.run(['ps','-p',str(pid),'-o','command='],capture_output=True,text=True).stdout.rstrip('\n')
        interpreter,separator,arguments=command.partition(' -m http.server ')
        suffix=' --directory '+directory
        if (separator and re.fullmatch(r'python(?:[0-9]+(?:\.[0-9]+)*)?',Path(interpreter).name,re.I)
                and arguments.endswith(suffix)
                and re.fullmatch(r'[0-9]+ --bind 127\.0\.0\.1',arguments[:-len(suffix)])):
            os.kill(pid,signal.SIGTERM)
except (OSError,ValueError,OverflowError): pass
PYEND
rm -rf "$OUT"
mkdir -p "$OUT/www/before-release" "$OUT/empty"

for build in "$BETA 2" "$RELEASE 3"; do
  read -r version number <<<"$build"
  echo "==> Building JEVision $version ($number), log in $OUT/build-$version.log"
  if ! JEVISION_BUILD_OUTPUT=$BUILD_OUTPUT JEVISION_VERSION=$version BUILD_NUMBER=$number scripts/build.sh >"$OUT/build-$version.log" 2>&1; then
    tail -20 "$OUT/build-$version.log" >&2
    exit 1
  fi
  mkdir -p "$OUT/$version" "$OUT/stage"
  ditto "$BUILD_OUTPUT/JEVision.app" "$OUT/$version/JEVision.app"
  ditto "$BUILD_OUTPUT/JEVision.app" "$OUT/stage/JEVision.app"
  hdiutil create -quiet -volname "JEVision $version" -srcfolder "$OUT/stage" -fs HFS+ -format ULFO "$OUT/www/JEVision-$version.dmg"
  rm -rf "$OUT/stage"
done

echo "==> Feeds"
printf 'Beta 1 of JEVision %s.\n' "$RELEASE" >"$OUT/beta.md"
printf 'Бета 1 JEVision %s.\n' "$RELEASE" >"$OUT/beta.ru.md"
RELEASE_NOTES=$OUT/beta.md RELEASE_NOTES_RU=$OUT/beta.ru.md DOWNLOAD_URL_PREFIX=$URL/ \
  scripts/make-appcast.sh "$OUT/www/JEVision-$BETA.dmg" "$OUT/empty"
cp build/appcast/appcast-beta.xml "$OUT/www/appcast-beta.xml"
cp build/appcast/appcast-beta.xml "$OUT/www/before-release/appcast-beta.xml"
DOWNLOAD_URL_PREFIX=$URL/ scripts/make-appcast.sh "$OUT/www/JEVision-$RELEASE.dmg" "$OUT/www"
cp build/appcast/appcast.xml build/appcast/appcast-beta.xml "$OUT/www/"
swift scripts/verify-appcast.swift "$OUT/www/appcast.xml" "$OUT/www/JEVision-$RELEASE.dmg" "$SPARKLE_PUBLIC_ED_KEY"
for version in "$BETA" "$RELEASE"; do
  swift scripts/verify-appcast.swift "$OUT/www/appcast-beta.xml" "$OUT/www/JEVision-$version.dmg" "$SPARKLE_PUBLIC_ED_KEY"
done
if grep -q "<sparkle:version>2</sparkle:version>" "$OUT/www/appcast.xml"; then
  echo "The release feed lists the beta." >&2
  exit 1
fi
sed 's#<title>JEVision</title>#<title>JEVision, changed</title>#' "$OUT/www/appcast.xml" >"$OUT/www/changed.xml"
python3 -c 'import re, sys; print(re.sub(r"\s*<!-- sparkle-signatures:.*?-->\s*$", "", open(sys.argv[1]).read(), flags=re.S))' \
  "$OUT/www/appcast.xml" >"$OUT/www/unsigned.xml"
cmp -s "$OUT/www/appcast.xml" "$OUT/www/changed.xml" && { echo "The changed feed came out the same." >&2; exit 1; }

echo "==> Update probe"
SPARKLE=$PWD/.build/artifacts/sparkle/Sparkle/Sparkle.xcframework/macos-arm64_x86_64
PROBE_APP=$OUT/UpdateProbe.app
mkdir -p "$PROBE_APP/Contents/MacOS"
/usr/libexec/PlistBuddy -c "Add :CFBundleIdentifier string io.github.dev-pikapik.jevision.update-probe" \
  -c "Add :CFBundleExecutable string update-probe" -c "Add :CFBundlePackageType string APPL" \
  -c "Add :CFBundleDevelopmentRegion string en" -c "Add :CFBundleLocalizations array" \
  -c "Add :CFBundleLocalizations:0 string en" -c "Add :CFBundleLocalizations:1 string ru" \
  "$PROBE_APP/Contents/Info.plist" >/dev/null
swiftc -swift-version 6 -O -F "$SPARKLE" -framework Sparkle -Xlinker -rpath -Xlinker "$SPARKLE" \
  scripts/update-probe.swift -o "$PROBE_APP/Contents/MacOS/update-probe"
PROBE=$PROBE_APP/Contents/MacOS/update-probe

SERVER_NONCE=$(python3 -c 'import secrets; print(secrets.token_hex(24))')
SERVER_NONCE_FILE=$OUT/www/.ready-$SERVER_NONCE
printf '%s' "$SERVER_NONCE" >"$SERVER_NONCE_FILE"
python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$OUT/www" >"$OUT/server.log" 2>&1 &
SERVER_PID=$!
echo "$SERVER_PID" >"$OUT/server.pid"
SERVER_READY=0
for _ in $(seq 50); do
  kill -0 "$SERVER_PID" 2>/dev/null || break
  # Another server can serve this directory too: require this child to own the listener.
  if [[ $(/usr/sbin/lsof -nP -a -p "$SERVER_PID" -iTCP:"$PORT" -sTCP:LISTEN -t 2>/dev/null || true) == "$SERVER_PID" ]] \
      && [[ $(curl --max-time 1 -fsS "$URL/.ready-$SERVER_NONCE" 2>/dev/null || true) == "$SERVER_NONCE" ]] \
      && kill -0 "$SERVER_PID" 2>/dev/null; then
    SERVER_READY=1
    break
  fi
  sleep 0.1
done
if [[ $SERVER_READY != 1 ]]; then
  echo "The update test server did not become ready on $URL." >&2
  tail -20 "$OUT/server.log" >&2
  exit 1
fi
rm -f "$SERVER_NONCE_FILE"
SERVER_NONCE_FILE=

BETA_APP=$OUT/$BETA/JEVision.app
RELEASE_APP=$OUT/$RELEASE/JEVision.app
DATA_FORMAT=$(/usr/libexec/PlistBuddy -c "Print :JEVisionDataFormat" "$RELEASE_APP/Contents/Info.plist")
failures=0
# check WHAT EXPECTED PROBE-ARGUMENTS…: the probe's answer must start with EXPECTED.
check() {
  local what=$1 expected=$2 got
  shift 2
  got=$("$PROBE" "$@" 2>&1 | tail -1)
  if [[ $got == "$expected"* ]]; then
    echo "  ok      $what: $got"
  else
    echo "  FAILED  $what: expected '$expected…', got '$got'"
    failures=$((failures + 1))
  fi
}
check "the beta is offered the release" "update $RELEASE 3 format=$DATA_FORMAT notes=" "$URL/appcast.xml" "$BETA_APP"
check "...on the beta channel too" "update $RELEASE 3 format=$DATA_FORMAT" "$URL/appcast-beta.xml" "$BETA_APP" beta
check "the release isn't offered itself again" "none up-to-date" "$URL/appcast.xml" "$RELEASE_APP"
check "the release isn't offered the older beta" "none newer-than-the-feed" "$URL/before-release/appcast-beta.xml" "$RELEASE_APP" beta
PROBE_BUILD=1 check "an earlier release gets no beta off the beta channel" "none up-to-date" "$URL/before-release/appcast-beta.xml" "$RELEASE_APP"
PROBE_BUILD=1 check "...and the beta on it" "update $BETA 2 format=$DATA_FORMAT notes=Beta 1" "$URL/before-release/appcast-beta.xml" "$RELEASE_APP" beta \
  -AppleLanguages "(en)"
check "notes in Russian on a Mac in Russian" "update $RELEASE 3 format=$DATA_FORMAT notes=Первый выпуск" "$URL/appcast.xml" "$BETA_APP" -AppleLanguages "(ru)"
check "notes in English on a Mac in English" "update $RELEASE 3 format=$DATA_FORMAT notes=The first release" "$URL/appcast.xml" "$BETA_APP" -AppleLanguages "(en)"
check "notes in English in another language" "update $RELEASE 3 format=$DATA_FORMAT notes=The first release" "$URL/appcast.xml" "$BETA_APP" -AppleLanguages "(de)"
check "a changed feed is refused" "refused" "$URL/changed.xml" "$BETA_APP"
check "an unsigned feed is refused" "refused" "$URL/unsigned.xml" "$BETA_APP"
PROBE_OTHER_KEY=1 check "a feed signed with another key is refused" "refused" "$URL/appcast.xml" "$BETA_APP"

cat <<EOF

The automatic server is stopped when this script ends. Feeds and test copies remain in $OUT.
For manual installation checks, use a separate macOS account: these copies have the normal
app identifier. Start a server there with:
  python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$OUT/www"
Then open the beta with JEVISION_UPDATE_FEED=$URL/appcast.xml and try Check for Updates….
EOF
if [ "$failures" -gt 0 ]; then
  echo "$failures check(s) failed." >&2
  exit 1
fi
echo "All checks passed."
