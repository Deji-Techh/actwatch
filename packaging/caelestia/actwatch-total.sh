#!/usr/bin/env bash
# actwatch — today's total as a single line of text.
#
# Use this when you want the number without writing any QML: polybar,
# waybar, a hyprland module, or a plain shell prompt.
#
#   actwatch-total.sh          -> "2h 14m"
#   actwatch-total.sh --json   -> the raw payload from `actwatch export`
#
# It always prints something and never hangs: a missing CLI, an empty
# database or a corrupt row all resolve to the "-" placeholder.
#
# From polybar (this DOES block, so polybar's `interval` matters):
#   [module/actwatch]
#   type = custom/script
#   interval = 300
#   exec = ~/.local/bin/actwatch-total.sh
set -uo pipefail

placeholder="-"

command -v actwatch >/dev/null 2>&1 || {
  printf '%s\n' "$placeholder"
  exit 0
}

payload="$(timeout 5 actwatch export --json --slim 2>/dev/null)" || payload=""
if [[ -z "$payload" ]]; then
  printf '%s\n' "$placeholder"
  exit 0
fi

if [[ "${1:-}" == "--json" ]]; then
  printf '%s\n' "$payload"
  exit 0
fi

# `export --json` emits a stable `totals.total_secs` field; pretty-printed as
# `"total_secs": 8040,`. Anything unparsable degrades to the placeholder.
secs="$(printf '%s\n' "$payload" | sed -n 's/.*"total_secs"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p' | head -1)"
if [[ -z "$secs" ]]; then
  printf '%s\n' "$placeholder"
  exit 0
fi

hours=$((secs / 3600))
minutes=$(((secs % 3600) / 60))

if (( hours == 0 )); then
  printf '%dm\n' "$minutes"
elif (( minutes == 0 )); then
  printf '%dh\n' "$hours"
else
  printf '%dh %dm\n' "$hours" "$minutes"
fi