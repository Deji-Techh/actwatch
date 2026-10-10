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

## Reading the database (actwatch-cli)

`actwatch` is strictly read-only: it opens the file with
`SQLITE_OPEN_READ_ONLY` and pins `PRAGMA query_only`. Only `actwatch-daemon`
writes. All of the CLI's SQL lives in one module,
`crates/actwatch-cli/src/sql.rs`, so the read contract is auditable in a
single place.

Two sources back the same row shape, because neither alone is sufficient:

- `daily_rollup` — cheap, carries `avg_cpu` / `peak_rss_kb` / `avg_gpu`, but
  lags for the current day.
- `app_events` — live, always current for today, no resource columns.

The default `--source auto` therefore reads **completed days from
`daily_rollup` and today live from `app_events`**. Rows record which table
produced them (`source` column in CSV/JSON), so the GUI and TUI can render
resource columns only where they exist instead of showing stale or empty data.

`ts` is unix seconds; day bucketing uses `date(ts,'unixepoch','localtime')` so
days follow the local timezone including DST. `day` in `daily_rollup` is
`YYYY-MM-DD` text, which sorts lexicographically.

## v2 contract (FROZEN — implement exactly, additive changes only)

All frontends (daemon writer, TUI, GUI, CLI) use `actwatch-core` for names,
categories, titles, and rollup reads. No local category/name maps.

### Display names — `core::names::DisplayNames`

- `DisplayNames::load()` scans `/usr/share/applications` + `~/.local/share/applications`
  for `*.desktop`, matching `StartupWMClass` (case-insensitive) and bare filename
  stem against the Hyprland app-id. Uses `Name=` on match.
- `resolve(app_id) -> String`: (1) `[aliases]` override from
  `~/.config/actwatch/categories.toml`, (2) desktop-file match, (3) prettify:
  strip reverse-DNS prefix (`com.t3tools.T3Code` → `T3Code`), replace `-_.`
  with spaces, Title-Case each word.
- Reserved id `"away"` (all idle/lock spans) resolves to `"Away"`.

### Categories — six, from one source

`core::Category::{Social, ProductivityFinance, Utilities, Gaming, Media, Other}`.
`Categories::categorize(app_id)` = `[categories]` TOML overrides, then builtins
(Steam/Lutris/Heroic/bottles → Gaming; vlc/mpv/obs/spotify → Media;
browsers/chat → Social; editors/terminals → Productivity & Finance;
settings/mixers/file managers → Utilities; else Other).
`DisplayNames` and `Categories` both read the same TOML; display labels:
"Social", "Productivity & Finance", "Utilities", "Gaming", "Media", "Other".

### Titles — `core::TitlePolicy::{Off, Hash, Full}` (default `Off`)

From `[privacy] titles = "off"|"hash"|"full"` in categories.toml.
`redact(title) -> Option<String>`: Off → None (store NULL), Hash → sha-like
hex (use std-only FNV/SipHash hex, no new deps), Full → Some(title).
UI is app-level only and never renders titles.

### Store batch + truthful rollups

- `Store::record_batch(&mut self, events: &[AppEvent], samples: &[ResourceSample])`
  — single transaction, lock-tolerant (busy_timeout 5000, return Err on lock,
  caller retains buffers).
- `Store::refresh_day(&self, day: &str)` — rebuilds that day's rollup rows from
  raw tables: `secs = SUM(dur_secs)`, `avg_cpu = AVG(cpu_pct)`,
  `peak_rss_kb = MAX(rss_kb)`, `avg_gpu = AVG(gpu_pct)`. Daemon calls it for the
  current day on every 60s commit. Frontends read `daily_rollup` only.
- `pub struct AppEvent { ts: i64, app_id: String, title: Option<String>, dur_secs: i64 }`
  and `ResourceSample { ts, app_id: String, pid: i64, cpu_pct: f64, rss_kb: i64, gpu_pct: f64 }`
  live in core (`core::model`) for all crates to share.

### Away + permissions (daemon)

- Idle = logind `IdleSinceHint` older than 120s (preferred, polled ≤ every 30s),
  fallback hypridle `listen` if present, then lockscreen-class only. Idle spans
  recorded under app_id `"away"`. Frontends exclude `"away"` from app totals
  and render a separate muted Away card/row.
- DB file created `0600`; daemon `chmod 0600`s existing file on open.

### B&W theme (TUI + GUI)

- Palette: background near-black `#0B0B0C`, surface `#141416`, text `#F4F4F5`,
  muted `#8E8E93`, hairlines `#262628`. No hue anywhere.
- Category series distinguished by shade + fill pattern (solid/dither/hatch
  block chars in TUI; alpha + stripe textures in iced). Legend shows swatch +
  label + total. Away always hatched/muted.
- Keep existing keybindings (TUI) and layout language (header total + WoW delta,
  7-day chart, category breakdown, sortable app table with friendly names,
  search). CLI output stays plain monochrome (no ANSI color).
