# actwatch in caelestia

Show today's total screen time in your bar without ever blocking the shell.

Three options, all read-only:

| File | Use when | Blocks? |
|---|---|---|
| `actwatch-pill/` | **recommended on caelestia-shell 2.x**: standalone monochrome pill, update-proof | no — async `Process`, 60s timer |
| `ActwatchTotal.qml` | you run a caelestia-shell fork with a custom bar slot | no — async `Process` |
| `actwatch-total.sh` | you want a one-line string (polybar, waybar, hyprland module) | yes, ~10ms; use it on a timer |

All three shell out to the same command:

```sh
actwatch export --json --slim
```

which defaults to **today** and prints a stable `totals.total_secs` field
(app time — Away is excluded, so the number is time actually at the PC).

---

## Option A — standalone pill (recommended)

caelestia-shell 2.x has a fixed bar entry set with no custom-module slot, so
injecting into its `Bar.qml` would break on every update. The pill sidesteps
that: its own `qs -c actwatch-pill` config, one small `PanelWindow`, zero
exclusive zone (it never moves your windows), monochrome to match actwatch.

```sh
mkdir -p ~/.config/quickshell/actwatch-pill
cp packaging/caelestia/actwatch-pill/shell.qml ~/.config/quickshell/actwatch-pill/
```

Add it to your session (Hyprland `exec-once`, alongside the shell itself):

```
exec-once = qs -c actwatch-pill
```

Top-right bar displays two monochrome pills:
1. `✎ Notes`: Click toggles the quick-capture Notes popup (~430x540) anchored below the bar.
2. `◷ 2h 14m`: Screen time pill. Left-click opens `actwatch-tui` in foot, right-click forces an immediate stats refresh. The first paint shows `—` until the CLI answers.

Remove it with `pkill -f "qs -c actwatch-pill"` and deleting the directory.

### Quick-Capture Notes Popup & Reminder Engine

The Notes popup provides fast, distraction-free capture with zero binary state:

#### Storage Layout (Obsidian-Compatible)
- **Daily Inbox**: `~/Notes/inbox/YYYY-MM-DD.md` (e.g. `~/Notes/inbox/2026-10-10.md`). Opening the popup auto-loads today's file and creates directories if needed.
- **Media Storage**: `~/Notes/media/` holds pasted images (`YYYYMMDD_HHMMSS.png`) and attached documents.
- **Relative Markdown Links**: Images are referenced as `![](media/<file>)` and attachments as `[<file>](media/<file>)`, making the notes vault directly compatible with Obsidian and standard markdown tooling.

#### Action Buttons & Controls
- **Save**: Writes the editor content directly to today's markdown file in `~/Notes/inbox/`.
- **Paste text**: Reads clipboard text via one-shot `wl-paste` and inserts at the cursor position or end of note.
- **Paste image**: Detects image data via `wl-paste --list-types`, streams clipboard to `~/Notes/media/<YYYYMMDD_HHMMSS>.png`, and inserts an inline markdown image tag.
- **Attach**: Small text field for any file path + Attach button. Copies the file into `~/Notes/media/` and appends a markdown link `[filename](media/filename)` (supports PDF, audio, video, images, or documents).
- **Copy**: One-click copy of the entire note (or currently selected text) to clipboard via `wl-copy` with a visual confirmation flash.
- **Remind**: Datetime field (`YYYY-MM-DD HH:MM`) + Remind button. Defaults to 1 minute in the future and takes text from the last line of the note, appending `- [ ] <datetime> <text>`.

#### Reminder Engine Syntax & Lifecycle
- **Reminder Syntax**:
  ```markdown
  - [ ] YYYY-MM-DD HH:MM <reminder message>
  ```
- **Scanning Engine**: Every 30 seconds, a background timer scans all `~/Notes/inbox/*.md` files for unnotified reminder items where the timestamp is due (`now >= scheduled_time`).
- **Desktop Notification**: Due reminders execute:
  ```sh
  notify-send -u critical "actwatch reminder" "<text>"
  ```
- **Zero Binary State**: Upon dispatch, the engine appends `#notified` to the line in the markdown file:
  ```markdown
  - [ ] YYYY-MM-DD HH:MM <reminder message> #notified
  ```
  Because state lives directly in the markdown file, reminders never double-fire across restarts and require no background daemon or external database.

---

## Option A — QML bar item (recommended)

Copy the snippet into your dotfiles:

```sh
cp packaging/caelestia/ActwatchTotal.qml \
   ~/dotfiles/caelestia-shell/src/modules/Bar/ActwatchTotal.qml
```

Then add it to your bar. In `caelestia-shell`, a bar item is normally a small
component listed in the bar's `leftItems` / `centerItems` / `rightItems`:

```qml
Bar {
    leftItems {
        ActwatchTotal { }
        Workspaces { }
    }
}
```

Reload the shell without restarting your session:

```sh
qs -c caelestia ipc call shell reload
```

### Why it cannot block your shell

- The CLI runs through Quickshell's `Process` with an async `StdioCollector`.
  There is no `$(...)` and no synchronous read anywhere.
- The value updates on a 1-minute timer; the bar paints immediately on every
  frame regardless of whether the CLI has answered yet.
- `root.ok` starts `false` and the label starts as `—`. If the CLI is missing,
  the run exits non-zero, the parser sees nothing, and the previous value stays
  on screen. The bar keeps rendering either way.
- `JSON.parse` is wrapped in `try/catch`: a database mid-write cannot throw an
  exception into the shell.

### Tuning

```qml
ActwatchTotal {
    refreshMinutes: 5      // default 1
}
```

Raise it if you care less about intraday accuracy. Lower it to 1 minute for a
live-ish counter.

### Shell version drift

The snippet uses only Quickshell built-ins (`Process`, `DataStreamParser`,
`Timer`, `Scope`), so it is independent of caelestia-shell's own component
API. If your shell version expects a different `leftItems` element type, keep
the body of `ActwatchTotal.qml` and wrap it in whatever your bar wants:

```qml
// minimal stand-alone window, handy for testing before wiring it into the bar
ShellRoot {
    ActwatchTotal { }
}
```

---

## Option B — shell one-liner

```sh
install -Dm755 packaging/caelestia/actwatch-total.sh ~/.local/bin/actwatch-total
actwatch-total.sh          # -> "2h 14m"
```

It prints `-` when the CLI is missing, the database is absent, or the payload
is unreadable, and it wraps the CLI in `timeout 5` so a wedged collector can
never hang your bar. Install it under a name your panel can call directly.

polybar:

```ini
[module/actwatch]
type = custom/script
interval = 300
exec = ~/.local/bin/actwatch-total
```

waybar (`modules-right`):

```json
"actwatch": {
  "exec": "~/.local/bin/actwatch-total 2>/dev/null",
  "interval": 300,
  "format": "{}"
}
```

Hyprland module (`hyprland.conf`), polled only while the module is visible:

```
actwatch {
  exec-once = ~/.local/bin/actwatch-total
  exec = echo -n ""; pgrep -x actwatch >/dev/null || ~/.local/bin/actwatch-total
  exec-continue = false
}
```

---

## Troubleshooting

**Label stays `—`.** Check the CLI directly:

```sh
actwatch status
actwatch export --json --slim | head
```

If `actwatch status` reports a missing database, the collector has never run:

```sh
systemctl --user enable --now actwatch-daemon.service
```

**`actwatch: command not found` in the bar but works in your terminal.**
The bar inherits the environment from the compositor session, not from your
shell. Put `actwatch` somewhere on the session `PATH` (`~/.local/bin` is the
usual) or set `environment: ["PATH=/home/you/.local/bin:/usr/bin"]` on the
component that spawns the process.

**Numbers look stale.** `refreshMinutes` defaults to 1; `export --json --slim`
defaults to today, so it should track the collector within a minute. If today's
row is missing entirely, the daemon is not running — check `actwatch status`.