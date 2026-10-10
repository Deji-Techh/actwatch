// actwatch pill — today's screen time as a tiny standalone widget.
//
// Why standalone: caelestia-shell 2.x has a fixed bar entry set (no custom
// module slot), so injecting into its Bar.qml would break on every update.
// This runs as its own `qs -c actwatch-pill` config: one small PanelWindow,
// zero exclusive zone (never moves your windows), monochrome to match the
// actwatch theme (bg #0B0B0C, text #F4F4F5, hairline #262628).
//
// Behavior mirrors ActwatchTotal.qml: async Process, 60s refresh, keeps the
// last value on failure, read-only CLI. Click opens the TUI (`foot -e
// actwatch-tui); right-click forces a refresh.

import Quickshell
import Quickshell.Io
import QtQuick

ShellRoot {
    id: root

    property string total: "—"
    property string sub: "actwatch"
    property bool ok: false

    readonly property list<string> command: ["actwatch", "export", "--json", "--slim"]

    PanelWindow {
        id: pill

        anchors {
            top: true
            right: true
        }
        margins {
            top: 8
            right: 8
        }
        exclusiveZone: 0

        implicitWidth: row.implicitWidth + 28
        implicitHeight: 34
        color: "transparent"

        Rectangle {
            anchors.fill: parent
            radius: 9
            color: "#D90B0B0C"
            border.width: 1
            border.color: "#262628"

            Row {
                id: row
                anchors.centerIn: parent
                spacing: 8

                Text {
                    text: "◷"
                    color: "#8E8E93"
                    font.pixelSize: 14
                    anchors.verticalCenter: parent.verticalCenter
                }
                Text {
                    text: root.total
                    color: "#F4F4F5"
                    font.pixelSize: 14
                    font.bold: true
                    anchors.verticalCenter: parent.verticalCenter
                }
            }

            MouseArea {
                anchors.fill: parent
                acceptedButtons: Qt.LeftButton | Qt.RightButton
                cursorShape: Qt.PointingHandCursor
                onClicked: (mouse) => {
                    if (mouse.button === Qt.RightButton) {
                        fetch.running = true;
                    } else {
                        launch.running = true;
                    }
                }
            }
        }
    }

    Process {
        id: fetch
        command: root.command
        running: false

        stdout: StdioCollector {
            onStreamFinished: root.ingest(text)
        }
        stderr: StdioCollector {}

        onExited: (exitCode, exitStatus) => {
            if (exitCode !== 0) {
                root.ok = false;
                root.sub = "actwatch unavailable";
            }
        }
    }

    // One-shot launcher for the TUI; reset so every click relaunches.
    Process {
        id: launch
        command: ["foot", "-e", "actwatch-tui"]
        running: false
        onExited: launch.running = false
    }

    Timer {
        interval: 60 * 1000
        running: true
        triggeredOnStart: true
        onTriggered: fetch.running = true
    }

    function ingest(text) {
        if (!text || text.length < 2)
            return;
        var data;
        try {
            data = JSON.parse(text);
        } catch (err) {
            return;
        }
        if (!data || !data.totals)
            return;
        root.total = format(data.totals.total_secs);
        root.ok = true;
        var apps = data.totals.apps || 0;
        root.sub = "actwatch · " + apps + " apps today";
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
