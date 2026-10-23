#!/bin/bash
# Makes the Sparkle update feeds for a release disk image made by release.sh. Doesn't publish anything.
#   make-appcast.sh build/JEVision-1.2.0.dmg [FROM]
# Two feeds: appcast.xml has the releases only; appcast-beta.xml, read by apps with "Get beta
# versions" on, has the releases and the betas (channel "beta"). A beta, a version such as
# 1.2.0-beta.1, goes into the beta feed only, a release into both. Each feed starts from the one in
# the folder FROM, or else from the published one, so earlier versions stay listed; FROM may be an
# empty folder to start new feeds.
# The new entry gets the release notes in English and in Russian (CHANGELOG.md, CHANGELOG.ru.md), the
# data format of the version (JEVisionDataFormat of its Info.plist, read by UpdateDataGuard.swift)
# and the EdDSA signature of the disk image; then the feed itself is signed, as the app accepts
# only signed feeds. Both are checked against the public key inside the app.
# Writes build/appcast/: publish appcast.xml as an asset of the release (the SUFeedURL of
# Resources/Info.plist), appcast-beta.xml as the asset of the pre-release "appcast-beta"
# (JEVisionBetaFeedURL), and the disk image as an asset of the release v<version>. See "Beta
# channel" in docs/maintainers/releasing.md.
#   SPARKLE_ED_KEY_FILE   file with the private update key, as `generate_keys -x` writes it
#                         (CI); default: the key in the Keychain
#   SPARKLE_KEY_ACCOUNT   Keychain account of the update key (default: jevision)
#   RELEASE_NOTES=file    release notes, .html or .md, to embed in the new entry (default: the
#                         section of CHANGELOG.md for this version)
#   RELEASE_NOTES_RU=file the same in Russian, .md (default: the section of CHANGELOG.ru.md)
#   DOWNLOAD_URL_PREFIX   where the disk image is downloaded from (default: the GitHub release
#                         v<version>); the update test bench points it at this Mac
set -euo pipefail

if [ $# -lt 1 ] || [ $# -gt 2 ] || [ ! -f "$1" ] || { [ $# -eq 2 ] && [ ! -d "$2" ]; }; then
  echo "Usage: $0 JEVision-<version>.dmg [folder with the current appcast.xml and appcast-beta.xml]" >&2
  exit 2
fi
DMG=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
FROM=${2:+$(cd "$2" && pwd)}
cd "$(dirname "$0")/.."

REPO=https://github.com/dev-pikapik/JEVision

# The version and the rest come from the app inside, not from the file name.
MOUNT=$(mktemp -d)
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$MOUNT" "$DMG"
INFO="$MOUNT/JEVision.app/Contents/Info.plist"
read_info() { plutil -extract "$1" raw -o - "$INFO" 2>/dev/null || true; }
VERSION=$(read_info CFBundleShortVersionString)
BUILD=$(read_info CFBundleVersion)
KEY=$(read_info SUPublicEDKey)
DATA_FORMAT=$(read_info JEVisionDataFormat)
FEED=$(read_info SUFeedURL)
BETA_FEED=$(read_info JEVisionBetaFeedURL)
hdiutil detach -quiet "$MOUNT"
rmdir "$MOUNT"
if [ -z "$KEY" ]; then
  echo "$1 was built without an update key, so it couldn't install updates: build it again with the key." >&2
  exit 1
fi
if ! [[ $DATA_FORMAT =~ ^[1-9][0-9]*$ ]] || [ -z "$FEED" ] || [ -z "$BETA_FEED" ]; then
  echo "$1 lacks JEVisionDataFormat, SUFeedURL or JEVisionBetaFeedURL in its Info.plist." >&2
  exit 1
fi

swift package resolve >/dev/null
SPARKLE_BIN=.build/artifacts/sparkle/Sparkle/bin
ACCOUNT=${SPARKLE_KEY_ACCOUNT:-jevision}
if [ -n "${SPARKLE_ED_KEY_FILE:-}" ]; then
  if [ ! -s "$SPARKLE_ED_KEY_FILE" ]; then
    echo "SPARKLE_ED_KEY_FILE $SPARKLE_ED_KEY_FILE is missing or empty." >&2
    exit 1
  fi
  SIGNING=(--ed-key-file "$SPARKLE_ED_KEY_FILE")
else
  # generate_appcast only warns without the private key, and its appcast would be useless.
  if ! public=$("$SPARKLE_BIN/generate_keys" --account "$ACCOUNT" -p 2>/dev/null) || [ "$public" != "$KEY" ]; then
    echo "The Keychain has no update key for account $ACCOUNT matching the one in $1." >&2
    exit 1
  fi
  SIGNING=(--account "$ACCOUNT")
fi

OUT=build/appcast
rm -rf "$OUT"
mkdir -p "$OUT"
NOTES_RU=$OUT/notes.ru.md
if [ -n "${RELEASE_NOTES_RU:-}" ]; then
  cp "$RELEASE_NOTES_RU" "$NOTES_RU"
else
  ../../scripts/changelog-section.sh "$VERSION" ru > "$NOTES_RU"
fi
FEEDS=(appcast-beta)
CHANNEL=(--channel beta)
if [[ $VERSION != *-* ]]; then
  FEEDS=(appcast appcast-beta)
  CHANNEL=()
fi

for name in "${FEEDS[@]}"; do
  url=$FEED
  [ "$name" = appcast ] || url=$BETA_FEED
  # generate_appcast reads and writes the feed in the folder of the archives, one folder per feed.
  work=$OUT/$name
  mkdir "$work"
  if [ -n "$FROM" ]; then
    [ ! -f "$FROM/$name.xml" ] || cp "$FROM/$name.xml" "$work/$name.xml"
  elif curl -fsL -o "$work/$name.xml" "$url"; then
    echo "==> Starting from $url"
  else
    rm -f "$work/$name.xml"
    echo "==> No feed at $url yet: starting a new one"
  fi
  cp -c "$DMG" "$work/"
  if [ -n "${RELEASE_NOTES:-}" ]; then
    cp "$RELEASE_NOTES" "$work/$(basename "${DMG%.dmg}").${RELEASE_NOTES##*.}"
  else
    ../../scripts/changelog-section.sh "$VERSION" > "$work/$(basename "${DMG%.dmg}").md"
  fi
  "$SPARKLE_BIN/generate_appcast" \
    "${SIGNING[@]}" \
    ${CHANNEL[@]+"${CHANNEL[@]}"} \
    --download-url-prefix "${DOWNLOAD_URL_PREFIX:-$REPO/releases/download/v$VERSION/}" \
    --link "$REPO" \
    --embed-release-notes \
    -o "$work/$name.xml" "$work"

  # Sparkle shows the description in the language of the Mac (xml:lang), the English one otherwise.
  # The feed is changed after generate_appcast signed it, so its signature is made again.
  python3 - "$work/$name.xml" "$BUILD" "$DATA_FORMAT" "$NOTES_RU" <<'EOF'
import re, sys
path, build, data_format, notes_ru = sys.argv[1:]
xml = open(path, encoding="utf-8").read()
xml = re.sub(r"\s*<!-- sparkle-signatures:.*?-->\s*$", "\n", xml, flags=re.S)
if "xmlns:jevision=" not in xml:
    xml = xml.replace("<rss ", '<rss xmlns:jevision="https://github.com/dev-pikapik/JEVision" ', 1)
items = re.split(r"(?=<item>)", xml)
found = [i for i, item in enumerate(items) if f"<sparkle:version>{build}</sparkle:version>" in item]
if len(found) != 1:
    sys.exit(f"{path} has {len(found)} entries with build {build}, not one")
item = items[found[0]]
if "<jevision:dataFormat>" not in item:
    notes = open(notes_ru, encoding="utf-8").read().replace("]]>", "]]]]><![CDATA[>")
    indent = re.search(r"\n(\s*)<enclosure", item).group(1)
    extra = (f'<description xml:lang="ru" sparkle:format="markdown"><![CDATA[{notes}]]></description>\n{indent}'
             f"<jevision:dataFormat>{data_format}</jevision:dataFormat>\n{indent}")
    items[found[0]] = item.replace("<enclosure", extra + "<enclosure", 1)
open(path, "w", encoding="utf-8").write("".join(items))
EOF
  "$SPARKLE_BIN/sign_update" "${SIGNING[@]}" "$work/$name.xml" >/dev/null
  mv "$work/$name.xml" "$OUT/"
  rm -rf "$work"
  swift scripts/verify-appcast.swift "$OUT/$name.xml" "$DMG" "$KEY"
done
rm "$NOTES_RU"

for name in "${FEEDS[@]}"; do echo "==> $PWD/$OUT/$name.xml (version $VERSION, build $BUILD)"; done
