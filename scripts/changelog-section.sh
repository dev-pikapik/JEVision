#!/bin/bash
# Prints one version's section of the changelog, without its heading: the release notes of that
# version, for the GitHub release and the update window.
#   scripts/changelog-section.sh 1.0.0          from CHANGELOG.md
#   scripts/changelog-section.sh 1.0.0 ru       from CHANGELOG.ru.md, the notes in Russian
set -euo pipefail
cd "$(dirname "$0")/.."

case "$#:${2:-en}" in
  1:en | 2:en) file=CHANGELOG.md ;;
  2:ru) file=CHANGELOG.ru.md ;;
  *) echo "usage: scripts/changelog-section.sh <version> [en|ru]" >&2; exit 2 ;;
esac
notes=$(awk -v heading="## [$1]" 'index($0, heading) == 1 { found = 1; next } found && /^## \[/ { exit } found && /^\[[^]]+\]: / { exit } found' "$file")
if [ -z "$(tr -d '[:space:]' <<<"$notes")" ]; then
  echo "$file has no notes for $1." >&2
  exit 1
fi
sed -e :a -e '/^[[:space:]]*$/{$d;N;ba' -e '}' <<<"$notes" | sed '/./,$!d'
