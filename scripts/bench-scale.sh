#!/bin/bash
# Measures how long the core takes on a made-up project of a thousand tests and thousands of
# saved runs, and writes one line per measurement, `name<TAB>milliseconds`, the fastest of three,
# then for the core `<TAB>milliseconds of processor time`, which a busy computer hardly changes.
#   scripts/bench-scale.sh                    the core, into build/bench-scale.tsv
#   scripts/bench-scale.sh --app              the window's paths too; needs scripts/build-core-apple.sh first
#   scripts/bench-scale.sh --out FILE         write FILE instead
#   scripts/bench-scale.sh --compare OLD.tsv  then show each measurement before, after and after / before,
#                                             by the clock and, where both files have it, processor time
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
APP_LOG="$LOG.app"
trap 'rm -f "$LOG" "$APP_LOG"' EXIT

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
  # A UI case owns an application/window lifecycle. Give each a fresh host process and
  # require a completed test summary: an early clean process exit is not a successful run.
  measure_app() {
    if ! env JEVISION_SCALE=1 swift test --package-path apps/macos -c release -Xswiftc -enable-testing \
      --no-parallel --filter "$1" >"$APP_LOG" 2>&1; then
      tail -40 "$APP_LOG" >&2
      exit 1
    fi
    if ! grep -Eq 'Test run with [1-9][0-9]* tests? .*passed' "$APP_LOG"; then
      tail -40 "$APP_LOG" >&2
      echo "The app benchmark runner ended without completing its tests." >&2
      exit 1
    fi
    cat "$APP_LOG" >>"$LOG"
  }
  measure_app ScaleBenchmarkTests
  for scenario in sidebar board largestTest longReport; do
    measure_app "ScaleUIBenchmarkTests.$scenario"
  done
fi

# A test's own output may follow the test runner's text on the same line.
grep -Eo $'BENCH\t[^\t]*\t[0-9.]+(\t[0-9.]+)?' "$LOG" | cut -f2- >"$OUT"
echo "==> $(wc -l <"$OUT" | tr -d ' ') measurements in $OUT"

if [[ -n $OLD ]]; then
  awk -F'\t' '
    function ratio(old, new) { return old != "" && old > 0 && new != "" ? sprintf("%.2f", new / old) : "-" }
    function shown(value) { return value != "" ? value : "-" }
    NR == FNR { before[$1] = $2; before_cpu[$1] = $3; next }
    FNR == 1 {
      printf "%-40s %12s %12s %8s %12s %12s %8s\n", "measurement", "before", "after", "ratio",
        "cpu before", "cpu after", "ratio"
    }
    {
      printf "%-40s %12s %12s %8s %12s %12s %8s\n", $1, shown(before[$1]), $2, ratio(before[$1], $2),
        shown(before_cpu[$1]), shown($3), ratio(before_cpu[$1], $3)
    }' "$OLD" "$OUT"
fi
