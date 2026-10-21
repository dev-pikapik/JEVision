#!/bin/bash
# Measures how long the core takes on a made-up project of a thousand tests and thousands of
# saved runs, and writes one line per measurement, `name<TAB>milliseconds`, the fastest of three.
#   scripts/bench-scale.sh                    the core, into build/bench-scale.tsv
#   scripts/bench-scale.sh --app              the window's paths too; needs scripts/build-core-apple.sh first
#   scripts/bench-scale.sh --out FILE         write FILE instead
#   scripts/bench-scale.sh --compare OLD.tsv  then show each measurement before, after and after / before
set -euo pipefail
cd "$(dirname "$0")/.."

usage() {
  echo "Usage: $0 [--app] [--out FILE] [--compare OLD.tsv]" >&2
  exit 2
}

APP=0
OUT=build/bench-scale.tsv
OLD=
while (($#)); do
  case $1 in
    --app) APP=1 ;;
    --out) (($# > 1)) || usage; OUT=$2; shift ;;
    --compare) (($# > 1)) || usage; OLD=$2; shift ;;
    *) usage ;;
  esac
  shift
done
if [[ -n $OLD && ! -f $OLD ]]; then
  echo "No such file to compare with: $OLD" >&2
  exit 2
fi

mkdir -p "$(dirname "$OUT")"
LOG=$(mktemp -t bench-scale)
trap 'rm -f "$LOG"' EXIT

# A failed measurement fails the script; its output is shown to tell why.
measure() {
  if ! "$@" >>"$LOG" 2>&1; then
    tail -40 "$LOG" >&2
    echo "Failed: $*" >&2
    exit 1
  fi
}

echo "==> Measuring the core"
measure cargo test --locked --release -q -p jevision-ffi --test scale_bench -- --ignored --nocapture --test-threads=1
if ((APP)); then
  if [[ ! -d build/apple/JEVisionCore ]]; then
    echo "The app needs the core built for Swift first: scripts/build-core-apple.sh" >&2
    exit 1
  fi
  echo "==> Measuring the app"
  measure env JEVISION_SCALE=1 swift test --package-path apps/macos -c release -Xswiftc -enable-testing \
    --filter ScaleBenchmark
fi

# A test's own output may follow the test runner's text on the same line.
grep -o $'BENCH\t[^\t]*\t[0-9.]*' "$LOG" | cut -f2- >"$OUT"
echo "==> $(wc -l <"$OUT" | tr -d ' ') measurements in $OUT"

if [[ -n $OLD ]]; then
  awk -F'\t' '
    NR == FNR { before[$1] = $2; next }
    FNR == 1 { printf "%-40s %12s %12s %8s\n", "measurement", "before", "after", "ratio" }
    {
      had = $1 in before
      ratio = had && before[$1] > 0 ? sprintf("%.2f", $2 / before[$1]) : "-"
      printf "%-40s %12s %12s %8s\n", $1, had ? before[$1] : "-", $2, ratio
    }' "$OLD" "$OUT"
fi
