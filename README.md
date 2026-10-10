<img src="assets/icon.svg" width="64" height="64" alt="actwatch icon">

# actwatch

24/7 activity viewer for Hyprland on Arch Linux. Tracks active-window usage plus
per-app **CPU / RAM / GPU**, stores it in local SQLite, and **keeps everything
forever** — there is no pruning anywhere in this codebase, by design.

Four binaries, one workspace, all Rust:

| Binary | What |
|---|---|
| `actwatch-daemon` | Event-driven collector (Hyprland IPC + `/proc` + Intel iGPU). ~1 MB RAM. Runs as a systemd user service. |
| `actwatch-tui` | Black-and-white terminal dashboard: 7-day chart, categories, sortable app table, search. |
| `actwatch-gui` | Black-and-white Wayland GUI (iced) with the same views. On-demand, not 24/7. |
| `actwatch` | Read-only CLI: `status`, `query`, `export --csv/--json`, `daemon` helpers. |

## Install

```sh
git clone https://github.com/Deji-Techh/actwatch.git && cd actwatch
cargo build --release --workspace
mkdir -p packaging/dist
cp target/release/actwatch target/release/actwatch-daemon \
   target/release/actwatch-gui target/release/actwatch-tui packaging/dist/
./packaging/install.sh
```

The installer puts binaries in `~/.local/bin`, units in
`~/.config/systemd/user`, a default `categories.toml` in
`~/.config/actwatch`, and creates `~/.local/share/actwatch/`.
`./packaging/install.sh --bins-only` skips systemd; `--no-service` installs
units without starting them.

### Verify

```sh
actwatch status
systemctl --user status actwatch-daemon.service
journalctl --user -u actwatch-daemon.service -f
```

## Use

```sh
actwatch status                      # db, schema, daemon, history span
actwatch query --week                # rolling 7 days, per day
actwatch query --day 2026-10-10      # one day, per app
actwatch query --app firefox         # one app, today
actwatch export --csv --week         # spreadsheets
actwatch export --json               # machine-readable (today by default)
actwatch-tui                         # terminal dashboard (q quits, / searches)
actwatch-gui-run                     # GUI (or actwatch-gui.service on demand)
```

App names come from installed `.desktop` files (`com.t3tools.T3Code` shows as
*T3 Code*), with `[aliases]` overrides in `~/.config/actwatch/categories.toml`.
Six categories: Social, Productivity & Finance, Utilities, Gaming, Media,
Other. Idle/locked time is recorded as **Away** — shown separately, never mixed
into app totals.

## 24/7 operation

The collector is a systemd **user** service: starts on login, restarts on
crash, follows your session, and is deliberately deprioritized
(`CPUWeight=10`, `Nice=10`, `MemoryMax=150M`) so it never competes with the
compositor. Sampling is event-driven (Hyprland socket, no per-second forks);
SQLite commits land every 60s in WAL mode.

```sh
systemctl --user enable --now actwatch-daemon.service   # start tracking
systemctl --user stop actwatch-daemon.service           # pause
systemctl --user disable actwatch-daemon.service        # stop at login
```

## Privacy

- Database is created `0600` (owner-only) at `~/.local/share/actwatch/`.
- Window titles are **not stored** by default (`[privacy] titles = "off"`;
  `"hash"` keeps groupable hashes, `"full"` keeps text). The UI is app-level
  only and never renders titles.
- Everything stays on disk. No network calls, no telemetry, no accounts.

## Uninstall

```sh
./packaging/uninstall.sh           # keeps your history
./packaging/uninstall.sh --purge   # also deletes data, config, docs
```

## Layout

```
crates/actwatch-core/     SQLite schema, display names, categories, rollups
crates/actwatch-daemon/   collector: Hyprland + resources + batched writes
crates/actwatch-tui/      ratatui dashboard (monochrome)
crates/actwatch-gui/      iced GUI (monochrome)
crates/actwatch-cli/      actwatch command
packaging/systemd/        user units (daemon always-on, GUI on-demand)
packaging/config/         default categories.toml
packaging/caelestia/      quickshell bar item for Caelestia
docs/ARCHITECTURE.md      schema, sampling policy, v2 contract
```

## License

MIT — see [LICENSE](LICENSE).
