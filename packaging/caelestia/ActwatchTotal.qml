// actwatch — today's total, as a caelestia bar item.
//
// Design constraints:
//   * Never blocks the shell: the CLI runs through Quickshell's Process,
//     which is asynchronous. No command substitution, no `$(...)` anywhere.
//   * Never blocks the shell even when the CLI is missing or slow: a failed
//     or empty run keeps the last known value and retries on the next tick.
//   * Read-only: `actwatch export --json --slim` only reads the database.
//
// Drop this into your caelestia-shell dotfiles, e.g.
//   caelestia-shell/src/modules/Bar/ActwatchTotal.qml
// and load it from your bar's leftItems / centerItems list.

import Quickshell
import Quickshell.Io

Scope {
    id: root

    // "2h 14m" once the first run lands, "—" before that.
    property string total: "—"
    property string tooltip: "actwatch"
    property bool ok: false

    readonly property list<string> command: ["actwatch", "export", "--json", "--slim"]

    readonly property int refreshMinutes: 1

    Process {
        id: proc
        command: root.command
        running: false

        stdout: StdioCollector {
            onStreamFinished: root.ingest(text)
        }

        stderr: StdioCollector {}

        onExited: (exitCode, exitStatus) => {
            if (exitCode !== 0) {
                root.ok = false;
                root.tooltip = "actwatch: CLI unavailable (exit " + exitCode + ")";
            }
        }
    }

    Timer {
        interval: root.refreshMinutes * 60 * 1000
        running: true
        triggeredOnStart: true
        onTriggered: proc.running = true
    }

    function ingest(text) {
        if (!text || text.length < 2)
            return;

        var data;
        try {
            data = JSON.parse(text);
        } catch (err) {
            // A partially flushed database should never break the bar.
            return;
        }

        if (!data || !data.totals)
            return;

        root.total = format(data.totals.total_secs);
        root.ok = true;
        root.tooltip = "actwatch · " + data.totals.apps + " apps today\n"
            + "last updated " + data.generated_at;
    }

    function format(secs) {
        secs = Math.floor(secs || 0);
        var h = Math.floor(secs / 3600);
        var m = Math.floor((secs % 3600) / 60);
        if (h <= 0)
            return m + "m";
        if (m <= 0)
            return h + "h";
        return h + "h " + m + "m";
    }
}