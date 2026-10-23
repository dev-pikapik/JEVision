#!/bin/bash
# Gives an app the macOS permissions that JEVision tests need (Screen Recording, Accessibility,
# and posting input events) on a GitHub-hosted macOS runner, where nobody can click "Allow".
#
#   scripts/ci-allow-ui-control.sh BUNDLE_ID APP_PATH…
#
# Only for a throwaway CI machine: it needs `sudo` without a password and a System Integrity
# Protection that is off, which hosted runners have. It refuses to run anywhere else, so it can't
# change the permissions of a real person's Mac.
#
# Two things are written:
#   the privacy database (TCC.db), for the system and for the runner's user, one row for each
#   permission, for the bundle identifier of the app;
#   the list of apps that may capture the screen without macOS asking again about its
#   "private window picker" (replayd's ScreenCaptureApprovals.plist), one key for each APP_PATH.
set -euo pipefail

if [ "${GITHUB_ACTIONS:-}" != true ] || [ "${RUNNER_ENVIRONMENT:-}" != github-hosted ]; then
  echo "This changes the privacy permissions of the whole Mac: it only runs on a GitHub-hosted runner." >&2
  exit 2
fi
if [ $# -lt 2 ]; then
  sed -n '2,7p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
fi
BUNDLE_ID=$1
shift

SYSTEM_DB="/Library/Application Support/com.apple.TCC/TCC.db"
USER_DB="$HOME/Library/Application Support/com.apple.TCC/TCC.db"
for db in "$SYSTEM_DB" "$USER_DB"; do
  if [ ! -f "$db" ]; then
    echo "$db doesn't exist: this macOS keeps its privacy database elsewhere, update this script." >&2
    exit 1
  fi
done

# auth_value 2 = allowed, auth_reason 3 = by the user; no csreq, so any build with this identifier
# matches (the probe on hosted runners showed that macOS accepts it).
for service in kTCCServiceScreenCapture kTCCServiceAccessibility kTCCServicePostEvent; do
  row="INSERT OR REPLACE INTO access
        (service,client,client_type,auth_value,auth_reason,auth_version,csreq,flags,last_modified)
        VALUES ('$service','$BUNDLE_ID',0,2,3,1,NULL,0,CAST(strftime('%s','now') AS INTEGER));"
  sudo sqlite3 "$SYSTEM_DB" "$row"
  sqlite3 "$USER_DB" "$row"
done

# A date far ahead says "asked already", so replayd doesn't raise its alert on every capture.
APPROVALS="$HOME/Library/Group Containers/group.com.apple.replayd/ScreenCaptureApprovals.plist"
mkdir -p "$(dirname "$APPROVALS")"
python3 -I - "$APPROVALS" "$@" <<'PY'
import datetime, plistlib, sys

path, *apps = sys.argv[1:]
try:
    with open(path, "rb") as f:
        approvals = plistlib.load(f)
except FileNotFoundError:
    approvals = {}
for app in apps:
    approvals["file://" + app.rstrip("/") + "/"] = datetime.datetime(3024, 1, 1)
with open(path, "wb") as f:
    plistlib.dump(approvals, f)
PY
# replayd keeps the approvals in memory and writes them back: it has to be restarted to reread them.
launchctl kickstart -k "gui/$(id -u)/com.apple.replayd" || killall -9 replayd || true
sleep 2
echo "Permissions of $BUNDLE_ID are set; screen capture approved for: $*"
