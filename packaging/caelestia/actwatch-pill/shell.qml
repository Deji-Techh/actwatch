// actwatch pill — today's screen time as a tiny standalone widget + quick-capture notes popup.
//
// Why standalone: caelestia-shell 2.x has a fixed bar entry set (no custom
// module slot), so injecting into its Bar.qml would break on every update.
// This runs as its own `qs -c actwatch-pill` config: pills in the top bar,
// zero exclusive zone (never moves your windows), monochrome to match the
// actwatch theme (bg #0B0B0C@85%, text #F4F4F5, hairline #262628).
//
// Notes popup anchors top-right (~430x540, below pills, exclusiveZone 0),
// bound to ~/Notes/inbox/YYYY-MM-DD.md and ~/Notes/media/.
// Reminders scanned every 30s and notified via notify-send.

import Quickshell
import Quickshell.Io
import QtQuick

ShellRoot {
    id: root

    property string total: "—"
    property string sub: "actwatch"
    property bool ok: false

    readonly property list<string> command: ["actwatch", "export", "--json", "--slim"]

    // Notes state
    readonly property string notesDir: Quickshell.env("HOME") + "/Notes"
    property string todayDate: getTodayDateString()
    property string todayFilePath: notesDir + "/inbox/" + todayDate + ".md"
    property string statusMsg: ""
    property bool copyFlashed: false

    function getTodayDateString() {
        var d = new Date();
        var y = d.getFullYear();
        var m = String(d.getMonth() + 1).padStart(2, '0');
        var day = String(d.getDate()).padStart(2, '0');
        return y + "-" + m + "-" + day;
    }

    function getDefaultReminderTime() {
        var d = new Date(Date.now() + 60 * 1000);
        var y = d.getFullYear();
        var m = String(d.getMonth() + 1).padStart(2, '0');
        var day = String(d.getDate()).padStart(2, '0');
        var hh = String(d.getHours()).padStart(2, '0');
        var mm = String(d.getMinutes()).padStart(2, '0');
        return y + "-" + m + "-" + day + " " + hh + ":" + mm;
    }

    function getLastLineText(fullText) {
        if (!fullText) return "Note reminder";
        var lines = fullText.split("\n");
        for (var i = lines.length - 1; i >= 0; i--) {
            var t = lines[i].trim();
            if (t.length > 0) {
                var cleaned = t.replace(/^-\s*\[\s*\]\s*/, '').replace(/^[-*]\s+/, '');
                if (cleaned.length > 0) return cleaned;
            }
        }
        return "Note reminder";
    }

    function flashStatus(msg) {
        root.statusMsg = msg;
        statusTimer.restart();
    }

    function toggleNotes() {
        notesPopup.visible = !notesPopup.visible;
        if (notesPopup.visible) {
            root.todayDate = getTodayDateString();
            root.todayFilePath = root.notesDir + "/inbox/" + root.todayDate + ".md";
            loadTodayNote();
            remindDtInput.text = getDefaultReminderTime();
        }
    }

    function loadTodayNote() {
        if (loadNoteProc.running) return;
        loadNoteProc.command = [
            "python3", "-c",
            "import sys, pathlib\n" +
            "p = pathlib.Path(sys.argv[1]).expanduser()\n" +
            "p.parent.mkdir(parents=True, exist_ok=True)\n" +
            "media = p.parent.parent / 'media'\n" +
            "media.mkdir(parents=True, exist_ok=True)\n" +
            "if not p.exists():\n" +
            "    p.touch()\n" +
            "print(p.read_text(encoding='utf-8'), end='')",
            root.todayFilePath
        ];
        loadNoteProc.running = true;
    }

    function saveNote() {
        if (saveNoteProc.running) return;
        saveNoteProc.command = [
            "python3", "-c",
            "import sys, pathlib\n" +
            "p = pathlib.Path(sys.argv[1]).expanduser()\n" +
            "p.parent.mkdir(parents=True, exist_ok=True)\n" +
            "p.write_text(sys.argv[2], encoding='utf-8')",
            root.todayFilePath,
            textEdit.text
        ];
        saveNoteProc.running = true;
    }

    function pasteText() {
        if (pasteTextProc.running) return;
        pasteTextProc.running = true;
    }

    function insertAtCursor(str) {
        if (textEdit.cursorPosition >= 0 && textEdit.cursorPosition <= textEdit.text.length) {
            var pos = textEdit.cursorPosition;
            textEdit.insert(pos, str);
            textEdit.cursorPosition = pos + str.length;
        } else {
            textEdit.text += str;
            textEdit.cursorPosition = textEdit.text.length;
        }
    }

    function pasteImage() {
        if (pasteImageProc.running) return;
        pasteImageProc.running = true;
    }

    function attachFile(path) {
        if (!path || path.trim().length === 0) {
            root.flashStatus("Enter a file path");
            return;
        }
        if (attachProc.running) return;
        attachProc.command = [
            "python3", "-c",
            "import sys, os, shutil, pathlib\n" +
            "src_str = sys.argv[1].strip()\n" +
            "src = pathlib.Path(src_str).expanduser()\n" +
            "if not src.is_file():\n" +
            "    print('NOT_FOUND')\n" +
            "    sys.exit(0)\n" +
            "media_dir = pathlib.Path('~/Notes/media').expanduser()\n" +
            "media_dir.mkdir(parents=True, exist_ok=True)\n" +
            "dest = media_dir / src.name\n" +
            "shutil.copy2(src, dest)\n" +
            "print(f'OK:{src.name}')",
            path.trim()
        ];
        attachProc.running = true;
    }

    function copyNote() {
        var toCopy = (textEdit.selectedText && textEdit.selectedText.length > 0)
            ? textEdit.selectedText
            : textEdit.text;
        if (!toCopy || toCopy.length === 0) {
            root.flashStatus("Nothing to copy");
            return;
        }
        if (copyProc.running) return;
        copyProc.command = ["wl-copy", toCopy];
        copyProc.running = true;
        root.copyFlashed = true;
        copyFlashTimer.restart();
        root.flashStatus("Copied to clipboard!");
    }

    function addReminder(inputStr, customText) {
        var raw = inputStr ? inputStr.trim() : "";
        var dt = "";
        var remText = customText ? customText.trim() : "";
        var m = raw.match(/^(\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}(?::\d{2})?)(?:\s+(.*))?$/);
        if (m) {
            dt = m[1].replace("T", " ");
            if (!remText && m[2]) {
                remText = m[2].trim();
            }
        } else if (raw.length > 0) {
            dt = raw;
        } else {
            dt = root.getDefaultReminderTime();
            remindDtInput.text = dt;
        }

        if (!remText) {
            remText = root.getLastLineText(textEdit.text);
        }

        var line = "- [ ] " + dt + " " + remText;
        var current = textEdit.text;
        if (current.length > 0 && !current.endsWith("\n")) {
            textEdit.text += "\n" + line + "\n";
        } else {
            textEdit.text += line + "\n";
        }
        textEdit.cursorPosition = textEdit.text.length;
        root.saveNote();
        root.flashStatus("Reminder added: " + dt);
    }

    function checkReminders() {
        if (!reminderProc.running)
            reminderProc.running = true;
    }

    // Top-right pills panel (Notes toggle + Screen time pill)
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

        implicitWidth: pillRow.implicitWidth
        implicitHeight: 34
        color: "transparent"

        Row {
            id: pillRow
            spacing: 8

            // Notes pill button
            Rectangle {
                id: notesBtn
                implicitWidth: notesRow.implicitWidth + 24
                implicitHeight: 34
                radius: 9
                color: notesPopup.visible ? "#FF18181A" : "#D90B0B0C"
                border.width: 1
                border.color: notesPopup.visible ? "#3F3F46" : "#262628"

                Row {
                    id: notesRow
                    anchors.centerIn: parent
                    spacing: 6

                    Text {
                        text: "✎"
                        color: "#8E8E93"
                        font.pixelSize: 13
                        anchors.verticalCenter: parent.verticalCenter
                    }
                    Text {
                        text: "Notes"
                        color: "#F4F4F5"
                        font.pixelSize: 14
                        font.bold: true
                        anchors.verticalCenter: parent.verticalCenter
                    }
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.toggleNotes()
                }
            }

            // Screen time pill button
            Rectangle {
                id: timeBtn
                implicitWidth: timeRow.implicitWidth + 28
                implicitHeight: 34
                radius: 9
                color: "#D90B0B0C"
                border.width: 1
                border.color: "#262628"

                Row {
                    id: timeRow
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
                            if (!fetch.running) fetch.running = true;
                        } else {
                            launch.running = true;
                        }
                    }
                }
            }
        }
    }

    // Notes Popup PanelWindow (~430x540, below the pills, exclusiveZone 0)
    PanelWindow {
        id: notesPopup
        visible: false
        focusable: true

        anchors {
            top: true
            right: true
        }
        margins {
            top: 48
            right: 8
        }
        exclusiveZone: 0

        implicitWidth: 430
        implicitHeight: 540
        color: "transparent"

        Rectangle {
            anchors.fill: parent
            radius: 12
            color: "#E60B0B0C"
            border.width: 1
            border.color: "#262628"
            clip: true

            Column {
                anchors.fill: parent
                anchors.margins: 14
                spacing: 10

                // Header
                Row {
                    width: parent.width
                    height: 24
                    spacing: 8

                    Text {
                        text: "Notes"
                        color: "#F4F4F5"
                        font.pixelSize: 15
                        font.bold: true
                        anchors.verticalCenter: parent.verticalCenter
                    }

                    Text {
                        text: "· " + root.todayDate + ".md"
                        color: "#8E8E93"
                        font.pixelSize: 12
                        font.family: "monospace"
                        anchors.verticalCenter: parent.verticalCenter
                    }

                    Item {
                        width: parent.width - 240
                        height: 1
                    }

                    Text {
                        text: root.statusMsg
                        color: "#4ADE80"
                        font.pixelSize: 11
                        font.bold: true
                        elide: Text.ElideRight
                        anchors.verticalCenter: parent.verticalCenter
                    }

                    Rectangle {
                        width: 22
                        height: 22
                        radius: 5
                        color: closeMouse.containsMouse ? "#27272A" : "transparent"
                        anchors.verticalCenter: parent.verticalCenter

                        Text {
                            text: "✕"
                            color: "#8E8E93"
                            font.pixelSize: 12
                            anchors.centerIn: parent
                        }

                        MouseArea {
                            id: closeMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: notesPopup.visible = false
                        }
                    }
                }

                // TextEdit Area
                Rectangle {
                    width: parent.width
                    height: 310
                    radius: 8
                    color: "#0E0E10"
                    border.width: 1
                    border.color: "#262628"
                    clip: true

                    Flickable {
                        id: flick
                        anchors.fill: parent
                        anchors.margins: 10
                        contentWidth: width
                        contentHeight: Math.max(height, textEdit.implicitHeight)
                        clip: true

                        TextEdit {
                            id: textEdit
                            width: flick.width
                            wrapMode: TextEdit.Wrap
                            color: "#F4F4F5"
                            font.pixelSize: 13
                            font.family: "monospace"
                            selectByMouse: true
                            selectionColor: "#3F3F46"
                            selectedTextColor: "#FFFFFF"
                            focus: true
                            cursorVisible: activeFocus

                            onCursorPositionChanged: {
                                var r = textEdit.cursorRectangle;
                                if (r.y < flick.contentY) {
                                    flick.contentY = r.y;
                                } else if (r.y + r.height > flick.contentY + flick.height) {
                                    flick.contentY = r.y + r.height - flick.height;
                                }
                            }
                        }
                    }
                }

                // Buttons row: Save | Paste text | Paste image | Attach | Copy | Remind
                Row {
                    width: parent.width
                    spacing: 6

                    // Save
                    Rectangle {
                        implicitWidth: saveLabel.implicitWidth + 16
                        implicitHeight: 28
                        radius: 6
                        color: saveMouse.containsMouse ? "#27272A" : "#18181A"
                        border.width: 1
                        border.color: saveMouse.containsMouse ? "#3F3F46" : "#262628"

                        Text {
                            id: saveLabel
                            anchors.centerIn: parent
                            text: "Save"
                            color: "#F4F4F5"
                            font.pixelSize: 12
                            font.bold: true
                        }

                        MouseArea {
                            id: saveMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.saveNote()
                        }
                    }

                    // Paste text
                    Rectangle {
                        implicitWidth: pasteTextLabel.implicitWidth + 14
                        implicitHeight: 28
                        radius: 6
                        color: pasteTextMouse.containsMouse ? "#27272A" : "#18181A"
                        border.width: 1
                        border.color: pasteTextMouse.containsMouse ? "#3F3F46" : "#262628"

                        Text {
                            id: pasteTextLabel
                            anchors.centerIn: parent
                            text: "Paste text"
                            color: "#F4F4F5"
                            font.pixelSize: 12
                            font.bold: true
                        }

                        MouseArea {
                            id: pasteTextMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.pasteText()
                        }
                    }

                    // Paste image
                    Rectangle {
                        implicitWidth: pasteImgLabel.implicitWidth + 14
                        implicitHeight: 28
                        radius: 6
                        color: pasteImgMouse.containsMouse ? "#27272A" : "#18181A"
                        border.width: 1
                        border.color: pasteImgMouse.containsMouse ? "#3F3F46" : "#262628"

                        Text {
                            id: pasteImgLabel
                            anchors.centerIn: parent
                            text: "Paste image"
                            color: "#F4F4F5"
                            font.pixelSize: 12
                            font.bold: true
                        }

                        MouseArea {
                            id: pasteImgMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.pasteImage()
                        }
                    }

                    // Attach
                    Rectangle {
                        implicitWidth: attachTopLabel.implicitWidth + 14
                        implicitHeight: 28
                        radius: 6
                        color: attachTopMouse.containsMouse ? "#27272A" : "#18181A"
                        border.width: 1
                        border.color: attachTopMouse.containsMouse ? "#3F3F46" : "#262628"

                        Text {
                            id: attachTopLabel
                            anchors.centerIn: parent
                            text: "Attach"
                            color: "#F4F4F5"
                            font.pixelSize: 12
                            font.bold: true
                        }

                        MouseArea {
                            id: attachTopMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                if (attachPathInput.text.trim().length > 0) {
                                    root.attachFile(attachPathInput.text);
                                } else {
                                    attachPathInput.forceActiveFocus();
                                    root.flashStatus("Type file path below");
                                }
                            }
                        }
                    }

                    // Copy
                    Rectangle {
                        implicitWidth: copyLabel.implicitWidth + 14
                        implicitHeight: 28
                        radius: 6
                        color: root.copyFlashed ? "#064E3B" : (copyMouse.containsMouse ? "#27272A" : "#18181A")
                        border.width: 1
                        border.color: root.copyFlashed ? "#10B981" : (copyMouse.containsMouse ? "#3F3F46" : "#262628")

                        Text {
                            id: copyLabel
                            anchors.centerIn: parent
                            text: root.copyFlashed ? "Copied!" : "Copy"
                            color: root.copyFlashed ? "#34D399" : "#F4F4F5"
                            font.pixelSize: 12
                            font.bold: true
                        }

                        MouseArea {
                            id: copyMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.copyNote()
                        }
                    }

                    // Remind
                    Rectangle {
                        implicitWidth: remindTopLabel.implicitWidth + 14
                        implicitHeight: 28
                        radius: 6
                        color: remindTopMouse.containsMouse ? "#27272A" : "#18181A"
                        border.width: 1
                        border.color: remindTopMouse.containsMouse ? "#3F3F46" : "#262628"

                        Text {
                            id: remindTopLabel
                            anchors.centerIn: parent
                            text: "Remind"
                            color: "#F4F4F5"
                            font.pixelSize: 12
                            font.bold: true
                        }

                        MouseArea {
                            id: remindTopMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                root.addReminder(remindDtInput.text, "");
                            }
                        }
                    }
                }

                // Attach field row: small TextField for a path + button
                Row {
                    width: parent.width
                    height: 28
                    spacing: 6

                    Rectangle {
                        width: parent.width - 76
                        height: 28
                        radius: 6
                        color: "#18181A"
                        border.width: 1
                        border.color: attachPathInput.activeFocus ? "#3F3F46" : "#262628"

                        TextInput {
                            id: attachPathInput
                            anchors.fill: parent
                            anchors.leftMargin: 8
                            anchors.rightMargin: 8
                            verticalAlignment: TextInput.AlignVCenter
                            color: "#F4F4F5"
                            font.pixelSize: 12
                            font.family: "monospace"
                            selectByMouse: true
                            clip: true

                            Text {
                                text: "Attach file: /path/to/media.png or pdf..."
                                color: "#71717A"
                                visible: !attachPathInput.text && !attachPathInput.activeFocus
                                anchors.fill: parent
                                verticalAlignment: Text.AlignVCenter
                                font: attachPathInput.font
                            }

                            onAccepted: root.attachFile(attachPathInput.text)
                        }
                    }

                    Rectangle {
                        width: 70
                        height: 28
                        radius: 6
                        color: attachBtnMouse.containsMouse ? "#27272A" : "#18181A"
                        border.width: 1
                        border.color: attachBtnMouse.containsMouse ? "#3F3F46" : "#262628"

                        Text {
                            anchors.centerIn: parent
                            text: "Attach"
                            color: "#F4F4F5"
                            font.pixelSize: 12
                            font.bold: true
                        }

                        MouseArea {
                            id: attachBtnMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.attachFile(attachPathInput.text)
                        }
                    }
                }

                // Remind field row: second small field YYYY-MM-DD HH:MM + button
                Row {
                    width: parent.width
                    height: 28
                    spacing: 6

                    Rectangle {
                        width: parent.width - 76
                        height: 28
                        radius: 6
                        color: "#18181A"
                        border.width: 1
                        border.color: remindDtInput.activeFocus ? "#3F3F46" : "#262628"

                        TextInput {
                            id: remindDtInput
                            anchors.fill: parent
                            anchors.leftMargin: 8
                            anchors.rightMargin: 8
                            verticalAlignment: TextInput.AlignVCenter
                            color: "#F4F4F5"
                            font.pixelSize: 12
                            font.family: "monospace"
                            selectByMouse: true
                            clip: true

                            Text {
                                text: "YYYY-MM-DD HH:MM"
                                color: "#71717A"
                                visible: !remindDtInput.text && !remindDtInput.activeFocus
                                anchors.fill: parent
                                verticalAlignment: Text.AlignVCenter
                                font: remindDtInput.font
                            }

                            onAccepted: root.addReminder(remindDtInput.text, "")
                        }
                    }

                    Rectangle {
                        width: 70
                        height: 28
                        radius: 6
                        color: remindBtnMouse.containsMouse ? "#27272A" : "#18181A"
                        border.width: 1
                        border.color: remindBtnMouse.containsMouse ? "#3F3F46" : "#262628"

                        Text {
                            anchors.centerIn: parent
                            text: "Remind"
                            color: "#F4F4F5"
                            font.pixelSize: 12
                            font.bold: true
                        }

                        MouseArea {
                            id: remindBtnMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.addReminder(remindDtInput.text, "")
                        }
                    }
                }
            }
        }
    }

    // IPC interface for scripting and external control
    IpcHandler {
        target: "notes"
        function toggle(): void {
            root.toggleNotes();
        }
        function save(): void {
            root.saveNote();
        }
        function pasteText(): void {
            root.pasteText();
        }
        function pasteImage(): void {
            root.pasteImage();
        }
        function copy(): void {
            root.copyNote();
        }
        function attach(path: string): void {
            root.attachFile(path);
        }
        function remind(dt: string, text: string): void {
            root.addReminder(dt, text);
        }
        function scanReminders(): void {
            root.checkReminders();
        }
        function setText(content: string): void {
            textEdit.text = content;
        }
        function getText(): void {
            // Can be read via status or clipboard
        }
        function setRemindDt(dt: string): void {
            remindDtInput.text = dt;
        }
    }

    // Processes
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

    Process {
        id: loadNoteProc
        running: false
        stdout: StdioCollector {
            onStreamFinished: {
                textEdit.text = text;
                root.flashStatus("Loaded " + root.todayDate + ".md");
            }
        }
        onExited: loadNoteProc.running = false
    }

    Process {
        id: saveNoteProc
        running: false
        onExited: {
            saveNoteProc.running = false;
            root.flashStatus("Saved " + root.todayDate + ".md");
        }
    }

    Process {
        id: pasteTextProc
        command: ["wl-paste", "--no-newline"]
        running: false
        stdout: StdioCollector {
            onStreamFinished: {
                if (text && text.length > 0) {
                    root.insertAtCursor(text);
                    root.flashStatus("Pasted text");
                } else {
                    root.flashStatus("Clipboard empty");
                }
            }
        }
        onExited: pasteTextProc.running = false
    }

    Process {
        id: pasteImageProc
        command: [
            "python3", "-c",
            "import subprocess, os, datetime, sys\n" +
            "env = os.environ.copy()\n" +
            "env.pop('LD_LIBRARY_PATH', None)\n" +
            "types = subprocess.run(['wl-paste', '--list-types'], capture_output=True, text=True, env=env).stdout\n" +
            "if 'image/' not in types:\n" +
            "    print('NO_IMAGE')\n" +
            "    sys.exit(0)\n" +
            "media_dir = os.path.expanduser('~/Notes/media')\n" +
            "os.makedirs(media_dir, exist_ok=True)\n" +
            "ts = datetime.datetime.now().strftime('%Y%m%d_%H%M%S')\n" +
            "filename = f'{ts}.png'\n" +
            "filepath = os.path.join(media_dir, filename)\n" +
            "with open(filepath, 'wb') as f:\n" +
            "    subprocess.run(['wl-paste'], stdout=f, check=True, env=env)\n" +
            "print(f'OK:{filename}')"
        ]
        running: false
        stdout: StdioCollector {
            onStreamFinished: {
                var out = text.trim();
                if (out.indexOf("OK:") === 0) {
                    var fn = out.substring(3).trim();
                    var link = "![](media/" + fn + ")";
                    var current = textEdit.text;
                    if (current.length > 0 && !current.endsWith("\n")) {
                        textEdit.text += "\n" + link + "\n";
                    } else {
                        textEdit.text += link + "\n";
                    }
                    textEdit.cursorPosition = textEdit.text.length;
                    root.saveNote();
                    root.flashStatus("Pasted image (media/" + fn + ")");
                } else {
                    root.flashStatus("Clipboard has no image");
                }
            }
        }
        onExited: pasteImageProc.running = false
    }

    Process {
        id: attachProc
        running: false
        stdout: StdioCollector {
            onStreamFinished: {
                var out = text.trim();
                if (out.indexOf("OK:") === 0) {
                    var fn = out.substring(3).trim();
                    var link = "[" + fn + "](media/" + fn + ")";
                    var current = textEdit.text;
                    if (current.length > 0 && !current.endsWith("\n")) {
                        textEdit.text += "\n" + link + "\n";
                    } else {
                        textEdit.text += link + "\n";
                    }
                    textEdit.cursorPosition = textEdit.text.length;
                    attachPathInput.text = "";
                    root.saveNote();
                    root.flashStatus("Attached " + fn);
                } else {
                    root.flashStatus("File not found");
                }
            }
        }
        onExited: attachProc.running = false
    }

    Process {
        id: copyProc
        running: false
        onExited: copyProc.running = false
    }

    Process {
        id: reminderProc
        command: [
            "python3", "-c",
            "import glob, os, re, datetime, subprocess\n" +
            "inbox_dir = os.path.expanduser('~/Notes/inbox')\n" +
            "if not os.path.isdir(inbox_dir):\n" +
            "    exit(0)\n" +
            "pattern = re.compile(r'^(\\s*-\\s*\\[\\s*\\]\\s*)(\\d{4}-\\d{2}-\\d{2}[ T]\\d{2}:\\d{2}(?::\\d{2})?)\\s+(.*?)$')\n" +
            "now = datetime.datetime.now()\n" +
            "env = os.environ.copy()\n" +
            "env.pop('LD_LIBRARY_PATH', None)\n" +
            "for filepath in glob.glob(os.path.join(inbox_dir, '*.md')):\n" +
            "    try:\n" +
            "        with open(filepath, 'r', encoding='utf-8') as f:\n" +
            "            lines = f.readlines()\n" +
            "    except Exception:\n" +
            "        continue\n" +
            "    modified = False\n" +
            "    new_lines = []\n" +
            "    for line in lines:\n" +
            "        if '#notified' in line:\n" +
            "            new_lines.append(line)\n" +
            "            continue\n" +
            "        stripped_line = line.rstrip('\\r\\n')\n" +
            "        m = pattern.match(stripped_line)\n" +
            "        if m:\n" +
            "            prefix, dt_str, reminder_text = m.groups()\n" +
            "            try:\n" +
            "                dt = datetime.datetime.fromisoformat(dt_str.replace(' ', 'T'))\n" +
            "                if now >= dt:\n" +
            "                    clean_text = reminder_text.strip()\n" +
            "                    subprocess.run(['notify-send', '-u', 'critical', 'actwatch reminder', clean_text], env=env)\n" +
            "                    line = f'{stripped_line} #notified\\n'\n" +
            "                    modified = True\n" +
            "            except Exception:\n" +
            "                pass\n" +
            "        new_lines.append(line)\n" +
            "    if modified:\n" +
            "        try:\n" +
            "            with open(filepath, 'w', encoding='utf-8') as f:\n" +
            "                f.writelines(new_lines)\n" +
            "        except Exception:\n" +
            "            pass\n"
        ]
        running: false
        onExited: reminderProc.running = false
    }

    // Timers
    Timer {
        interval: 60 * 1000
        running: true
        repeat: true
        triggeredOnStart: true
        onTriggered: {
            if (!fetch.running)
                fetch.running = true;
        }
    }

    Timer {
        id: reminderTimer
        interval: 30 * 1000
        running: true
        repeat: true
        triggeredOnStart: true
        onTriggered: {
            if (!reminderProc.running)
                reminderProc.running = true;
        }
    }

    Timer {
        id: statusTimer
        interval: 3000
        repeat: false
        onTriggered: root.statusMsg = ""
    }

    Timer {
        id: copyFlashTimer
        interval: 1000
        repeat: false
        onTriggered: root.copyFlashed = false
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
