<img src="assets/icon.svg" width="64" height="64" alt="">

# activitydisplay

Created in [T3 Code](https://t3.codes).

24/7 activity viewer for Hyprland on Arch. Tracks active-window usage plus
per-app CPU / RAM / GPU into SQLite and **keeps everything forever** — there is
no pruning anywhere in this codebase, by design.

- Design and schema: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- Bar-panel integration: [packaging/caelestia/README.md](packaging/caelestia/README.md)

---

## Install

```sh
git clone <this-repo> && cd activitydisplay

cargo build --release --workspace
mkdir -p packaging/dist
cp target/release/actwatch target/release/actwatch-daemon \
   target/release/actwatch-gui target/release/actwatch-tui packaging/dist/ 2>/dev/null

./packaging/install.sh
```

The installer puts binaries in `~/.local/bin`, units in
`~/.config/systemd/user`, a default `categories.toml` in
`~/.config/actwatch`, and creates `~/.local/share/actwatch/` (the daemon runs
with `ProtectHome=read-only`, so that directory must exist before it starts).

Run `./packaging/install.sh --help` for the `--no-service` and `--bins-only`
variants.

### Verify

```sh
actwatch status              # database, schema and daemon health
systemctl --user status actwatch-daemon.service
journalctl --user -u actwatch-daemon.service -f
```

## Run 24/7

The collector is a systemd **user** service. It starts on login, restarts on
crash, follows your session (logging out stops it) and is deliberately
low-priority so it never competes with the compositor.

```sh
systemctl --user enable --now actwatch-daemon.service   # install + start
systemctl --user status actwatch-daemon.service
systemctl --user stop actwatch-daemon.service            # pause collection
systemctl --user disable actwatch-daemon.service         # stop starting at login
```

Useful knobs, all overridable without editing the shipped unit:

```sh
systemctl --user edit actwatch-daemon.service
```

The unit ships `CPUWeight=10`, `Nice=10`, `MemoryMax=150M`, `Restart=always`
and `RestartSec=5s`. The optional GUI is `actwatch-gui.service` (`Restart=on-failure`, so a crash
never turns into a restart storm). It starts through `actwatch-gui-run`, a
small wrapper that resolves the Wayland socket — `systemd --user` does not
reliably inherit the compositor's environment, and the socket name is not
fixed (Hyprland uses `wayland-1`, not `wayland-0`):

```sh
systemctl --user start actwatch-gui.service
systemctl --user enable --now actwatch-gui.service
```

## CLI

`actwatch` is read-only: it opens the database with `SQLITE_OPEN_READ_ONLY`
and pins `PRAGMA query_only`. Only `actwatch-daemon` ever writes.

```sh
actwatch status                      # health: db, schema, daemon, history span
actwatch query --day                 # today so far, per app
actwatch query --week                # rolling 7 days, per day
actwatch query --app firefox         # one app, today
actwatch export --csv --week         # spreadsheet rows
actwatch export --json --slim        # machine-readable rows (today by default)
actwatch daemon start                # systemctl --user start actwatch-daemon
```

Range flags combine: `--day YYYY-MM-DD`, `--week`, `--since`, `--until`,
plus `--app` and `--source {auto,rollup,events}`. `auto` (the default) reads
completed days from `daily_rollup` — which carries the CPU/RAM/GPU columns —
and today live from `app_events`, so the current day is never stale.

## Categories

`packaging/config/categories.toml` is the shipped default, installed to
`~/.config/actwatch/categories.toml`. Matching is case-insensitive substring
against the window class / process name; anything unmatched falls back to
`default_category`.

```toml
version = 2
default_category = "utilities"

[[category]]
id = "social"
name = "Social"
color = "#7c5cff"
order = 1
patterns = ["firefox", "discord", "telegram"]
```

The defaults ship five categories — Social, Productivity, Utilities, Finance
and Terminal & Tiling — which is a superset of the three shown in the original
mockup.

## Uninstall

```sh
./packaging/uninstall.sh
```

This stops and disables both services, removes the units, the binaries and the
docs — and **keeps your history** in `~/.local/share/actwatch/`. Because
actwatch never prunes, the database is only ever removed when you explicitly ask:

```sh
./packaging/uninstall.sh --purge   # also deletes data, config and docs
```

Manual equivalent:

```sh
systemctl --user disable --now actwatch-daemon.service
rm -f ~/.config/systemd/user/actwatch-{daemon,gui}.service
rm -f ~/.local/bin/actwatch{,-daemon,-gui,-gui-run,-tui}
systemctl --user daemon-reload
# optional, destructive:
rm -rf ~/.local/share/actwatch ~/.config/actwatch
```

## Layout

```
crates/actwatch-cli/        actwatch command + systemd-facing helpers
packaging/systemd/          actwatch-daemon.service, actwatch-gui.service
packaging/run-gui.sh        resolves the Wayland socket for the GUI unit
packaging/config/           default categories.toml
packaging/caelestia/        quickshell bar item + shell fallback
packaging/install.sh        installer
packaging/uninstall.sh      uninstaller (--purge to drop history)
docs/ARCHITECTURE.md        schema and resource policy
```