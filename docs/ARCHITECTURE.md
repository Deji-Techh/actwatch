# actwatch — 24/7 activity viewer for Hyprland (Arch)

Tracks active-window usage + per-app CPU / RAM / GPU, stored forever in SQLite.
Native Wayland GUI (iced) + TUI (ratatui), modelled on the Activity screenshot:
daily stacked bar (S–S), This/Today navigation, category legend
(Social / Productivity & Finance / Utilities), app table (Apps / Time / Limits)
with search, plus resource columns (avg CPU %, peak RSS, GPU %).

## Crates (each built on its own branch, merged when done)

| Crate | Branch | Owner thread |
|---|---|---|
| `actwatch-core` — SQLite schema, categories, aggregation queries, retention-forever policy | `feat/core-store` | store thread |
| `actwatch-daemon` — Hyprland IPC event listener + /proc + Intel i915 sampler, WAL batched writes, <15MB RSS target | `feat/collector-daemon` | collector thread |
| `actwatch-tui` — ratatui dashboard (daily chart + app table + resources) | `feat/tui` | tui thread |
| `actwatch-gui` — iced GUI matching screenshot, runs 24/7 friendly | `feat/gui` | gui thread |
| `actwatch-cli` + systemd units + caelestia hook | `feat/packaging` | packaging thread |

## Sampling / resource policy (low CPU+RAM)

- Active window: event-driven via Hyprland socket2 (`$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock`),
  fallback poll `hyprctl clients -j` every 2s. No per-second forks in steady state.
- Resources: read `/proc/<pid>/stat` + `status` every 5s for tracked PIDs only;
  Intel GPU via `/sys/class/drm/card*/engine/*/busy_time` deltas (no `intel_gpu_top` fork).
- DB: single SQLite file `~/.local/share/actwatch/activity.db`, WAL mode,
  60s batched commits, monthly `VACUUM`. No pruning — history kept always.
- Daemon target: <15MB RSS, <0.5% CPU on 4-core i5 (this machine).

## Schema (v1, see actwatch-core)

- `app_events(ts INTEGER, app_id TEXT, title TEXT, dur_secs INTEGER)`
- `resource_samples(ts INTEGER, app_id TEXT, pid INTEGER, cpu_pct REAL, rss_kb INTEGER, gpu_pct REAL)`
- `daily_rollup(day TEXT, app_id TEXT, secs INTEGER, avg_cpu REAL, peak_rss_kb INTEGER, avg_gpu REAL)`
- `meta(key TEXT PRIMARY KEY, value TEXT)` — schema_version, category map version
