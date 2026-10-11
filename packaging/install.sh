#!/usr/bin/env bash
# actwatch installer: binaries, units and default config.
#
#   ./packaging/install.sh              install everything
#   ./packaging/install.sh --no-service install but do not touch systemd
#   ./packaging/install.sh --bins-only  only install the binaries
#
# Binaries are expected at packaging/dist/ (see `cargo build --release`).
set -euo pipefail

BINDIR="${BINDIR:-$HOME/.local/bin}"
UNITDIR="${UNITDIR:-$HOME/.config/systemd/user}"
CONFDIR="${CONFDIR:-$HOME/.config/actwatch}"
DATADIR="${DATADIR:-$HOME/.local/share/actwatch}"
DOCDIR="${DOCDIR:-$HOME/.local/share/doc/actwatch}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="${DIST:-$REPO_ROOT/packaging/dist}"

WITH_SERVICE=1
ONLY_BINS=0

log()  { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarn:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

for arg in "$@"; do
  case "$arg" in
    --no-service) WITH_SERVICE=0 ;;
    --bins-only)  ONLY_BINS=1 ;;
    -h|--help)    sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown option: $arg" ;;
  esac
done

[[ -d "$DIST" ]] || die "no build output at $DIST
build the workspace first:
  cargo build --release --workspace
  mkdir -p packaging/dist
  cp target/release/actwatch{,-daemon,-gui,-tui} packaging/dist/ 2>/dev/null || true"

log "installing binaries into $BINDIR"
mkdir -p "$BINDIR"
installed=0
for bin in actwatch actwatch-daemon actwatch-gui actwatch-tui; do
  if [[ -f "$DIST/$bin" ]]; then
    install -Dm755 "$DIST/$bin" "$BINDIR/$bin"
    log "  $bin"
    installed=$((installed + 1))
  fi
done
(( installed > 0 )) || die "found no binaries in $DIST"

log "installing the GUI launcher and backup helper"
install -Dm755 "$REPO_ROOT/packaging/run-gui.sh" "$BINDIR/actwatch-gui-run"
install -Dm755 "$REPO_ROOT/packaging/actwatch-backup.sh" "$BINDIR/actwatch-backup"

log "creating data directory $DATADIR"
# The daemon unit runs with ProtectHome=read-only, so it cannot create this
# directory itself; it must exist before the service starts.
mkdir -p "$DATADIR"
chmod 700 "$DATADIR"

log "installing config"
mkdir -p "$CONFDIR" "$DOCDIR"
if [[ -f "$REPO_ROOT/packaging/config/categories.toml" ]]; then
  if [[ -f "$CONFDIR/categories.toml" ]]; then
    cp "$CONFDIR/categories.toml" "$CONFDIR/categories.toml.bak"
    warn "existing $CONFDIR/categories.toml backed up to .bak"
  fi
  install -Dm644 "$REPO_ROOT/packaging/config/categories.toml" "$CONFDIR/categories.toml"
fi
install -Dm644 "$REPO_ROOT/docs/ARCHITECTURE.md" "$DOCDIR/ARCHITECTURE.md"
if [[ -f "$REPO_ROOT/README.md" ]]; then
  install -Dm644 "$REPO_ROOT/README.md" "$DOCDIR/README.md"
fi

if (( ONLY_BINS )); then
  log "done (bins only). enable the service later with:"
  echo "  systemctl --user enable --now actwatch-daemon.service"
  exit 0
fi

log "installing systemd user units into $UNITDIR"
mkdir -p "$UNITDIR"
for unit in "$REPO_ROOT"/packaging/systemd/*.service "$REPO_ROOT"/packaging/systemd/*.timer; do
  [[ -f "$unit" ]] || continue
  install -Dm644 "$unit" "$UNITDIR/$(basename "$unit")"
  log "  $(basename "$unit")"
done

if command -v systemctl >/dev/null 2>&1 && (( WITH_SERVICE )); then
  log "reloading user systemd"
  systemctl --user daemon-reload
  if systemctl --user is-enabled --quiet actwatch-daemon.service 2>/dev/null; then
    log "actwatch-daemon.service already enabled; restarting"
    systemctl --user restart actwatch-daemon.service
  else
    log "enabling actwatch-daemon.service (24/7 collector)"
    systemctl --user enable --now actwatch-daemon.service
  fi
  if systemctl --user is-enabled --quiet actwatch-backup.timer 2>/dev/null; then
    log "actwatch-backup.timer already enabled"
  else
    log "enabling actwatch-backup.timer (weekly DB backup)"
    systemctl --user enable --now actwatch-backup.timer
  fi
  if systemctl --user is-enabled --quiet actwatch-pill.service 2>/dev/null; then
    log "actwatch-pill.service already enabled; restarting"
    systemctl --user restart actwatch-pill.service
  else
    log "enabling actwatch-pill.service (notes + screen-time pills)"
    systemctl --user enable --now actwatch-pill.service
  fi
  log "status:"
  systemctl --user --no-pager status actwatch-daemon.service || true
else
  warn "systemd user session not available; enable the unit manually:"
  echo "  systemctl --user daemon-reload"
  echo "  systemctl --user enable --now actwatch-daemon.service"
fi

log "installed. try:"
echo "  actwatch status"
echo "  actwatch query --week"
echo "  systemctl --user start actwatch-backup.service   # first backup now"