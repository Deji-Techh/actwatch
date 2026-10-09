#!/usr/bin/env bash
# Launch actwatch-gui from a systemd user unit.
#
# systemd --user does not reliably inherit the compositor session's
# environment (it is commonly started before the compositor), and the
# Wayland socket name is not fixed: Hyprland uses wayland-1 by default, but
# a second session or a nested compositor can change it. Guessing here breaks
# the GUI with a confusing "cannot connect to display".
#
# So: use WAYLAND_DISPLAY when it is set, otherwise discover the socket.
set -euo pipefail

runtime="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
gui="${ACTWATCH_GUI:-$HOME/.local/bin/actwatch-gui}"

if [[ -n "${WAYLAND_DISPLAY:-}" ]]; then
  # An explicit value is trusted as-is; unusual setups may legitimately point
  # somewhere this script cannot introspect.
  :
else
  WAYLAND_DISPLAY=""
  for candidate in "$runtime"/wayland-*; do
    [[ -S "$candidate" ]] || continue
    WAYLAND_DISPLAY="$(basename "$candidate")"
    break
  done
fi

if [[ -z "${WAYLAND_DISPLAY:-}" ]]; then
  echo "actwatch-gui: no Wayland socket in $runtime" >&2
  echo "hint: start this from inside your graphical session, or set WAYLAND_DISPLAY" >&2
  exit 1
fi

export WAYLAND_DISPLAY
export XDG_DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
export XDG_RUNTIME_DIR="$runtime"

exec "$gui" "$@"