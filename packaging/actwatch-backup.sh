#!/usr/bin/env bash
# actwatch-backup — weekly timestamped backup of the activity database.
#
# Uses SQLite's online backup API, so it is safe while actwatch-daemon is
# writing. Verifies the copy with `PRAGMA integrity_check`, locks it to 0600
# and keeps the newest $KEEP copies (default 8 ≈ two months of weeklies).
#
# Restore: stop the daemon, copy the backup over activity.db, start it:
#   systemctl --user stop actwatch-daemon.service
#   cp ~/.local/share/actwatch/backups/activity-<stamp>.db \
#      ~/.local/share/actwatch/activity.db
#   systemctl --user start actwatch-daemon.service
set -euo pipefail

DATADIR="${DATADIR:-$HOME/.local/share/actwatch}"
BACKUPDIR="$DATADIR/backups"
KEEP="${KEEP:-8}"
DB="$DATADIR/activity.db"

log() { printf 'actwatch-backup: %s\n' "$*" >&2; }

command -v sqlite3 >/dev/null 2>&1 || {
  log "sqlite3 CLI not found (Arch: pacman -S sqlite)"
  exit 1
}
[[ -f "$DB" ]] || {
  log "no database yet at $DB, nothing to back up"
  exit 0
}

mkdir -p "$BACKUPDIR"
chmod 700 "$BACKUPDIR"

stamp="$(date +%Y%m%d-%H%M%S)"
tmp="$BACKUPDIR/.tmp-$stamp.db"
final="$BACKUPDIR/activity-$stamp.db"

sqlite3 "$DB" ".backup '$tmp'"
integrity="$(sqlite3 "$tmp" "PRAGMA integrity_check;")"
if [[ "$integrity" != "ok" ]]; then
  log "integrity_check failed on fresh copy, discarding"
  rm -f "$tmp"
  exit 1
fi
chmod 600 "$tmp"
mv -f "$tmp" "$final"

# Prune oldest beyond $KEEP (newest-first listing, skip KEEP, delete rest).
# shellcheck disable=SC2012
ls -1t "$BACKUPDIR"/activity-*.db 2>/dev/null \
  | tail -n "+$((KEEP + 1))" \
  | xargs -r rm -f

log "wrote $final (keeping newest $KEEP)"
