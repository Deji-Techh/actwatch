#!/usr/bin/env bash
# actwatch uninstaller.
#
#   ./packaging/uninstall.sh              remove everything except the database
#   ./packaging/uninstall.sh --purge      also delete the database and config
#
# actwatch never prunes history on its own, so the database survives an
# ordinary uninstall. Deleting it is only ever explicit (--purge).
set -euo pipefail

BINDIR="${BINDIR:-$HOME/.local/bin}"
UNITDIR="${UNITDIR:-$HOME/.config/systemd/user}"
CONFDIR="${CONFDIR:-$HOME/.config/actwatch}"
DATADIR="${DATADIR:-$HOME/.local/share/actwatch}"
DOCDIR="${DOCDIR:-$HOME/.local/share/doc/actwatch}"

PURGE=0

log()  { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarn:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

for arg in "$@"; do
  case "$arg" in
    --purge) PURGE=1 ;;
    -h|--help) sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown option: $arg" ;;
  esac
done

for unit in actwatch-gui.service actwatch-daemon.service actwatch-backup.timer; do
  if command -v systemctl >/dev/null 2>&1; then
    if systemctl --user is-active --quiet "$unit" 2>/dev/null; then
      log "stopping $unit"
      systemctl --user stop "$unit"
    fi
    if systemctl --user is-enabled --quiet "$unit" 2>/dev/null; then
      log "disabling $unit"
      systemctl --user disable "$unit"
    fi
  fi
done

if command -v systemctl >/dev/null 2>&1; then
  log "reloading user systemd"
  systemctl --user daemon-reload || true
fi

log "removing units"
rm -fv "$UNITDIR/actwatch-daemon.service" "$UNITDIR/actwatch-gui.service" \
  "$UNITDIR/actwatch-backup.service" "$UNITDIR/actwatch-backup.timer"

log "removing binaries"
for bin in actwatch actwatch-daemon actwatch-gui actwatch-gui-run actwatch-tui actwatch-backup; do
  rm -fv "$BINDIR/$bin"
done

rmdir "$UNITDIR" 2>/dev/null || true

if (( PURGE )); then
  log "purging data, config and docs"
  rm -rfv "$DATADIR" "$CONFDIR" "$DOCDIR"
  log "history deleted"
else
  rm -f "$DOCDIR/README.md" "$DOCDIR/ARCHITECTURE.md"
  rmdir "$DOCDIR" 2>/dev/null || true
  warn "kept your history in $DATADIR"
  warn "delete it with: ./packaging/uninstall.sh --purge"
fi

log "done. re-install with ./packaging/install.sh"