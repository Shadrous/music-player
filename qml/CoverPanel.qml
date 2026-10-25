import QtQuick
import QtQuick.Effects
import QtQuick.Layouts
import spinwave

// Art pane (360px, radius 28): the current cover, or the design's spinning
// M3 shapes when there is no art, with the queue-position chip. With lyrics
// on, the cover blurs under a dark tint and the song's lyrics scroll on top.
// Layout only.
Rectangle {
    id: pane
    radius: 28
    clip: true
    color: Theme.primaryContainer

    readonly property bool hasArt: art.ready
    readonly property bool motion: Backend.isPlaying && !Backend.lowPerformance
    property bool lyricsOn: false
    // Parsed lyrics ({lines, source}); null while loading / off.
    property var lyrics: null

    function loadLyrics() {
        if (!pane.lyricsOn || Backend.videoId.length === 0) {
            lyricsPoll.stop()
            pane.lyrics = null
            return
        }
        var raw = Backend.lyricsJson(Backend.videoId)
        if (raw.length === 0) {
            pane.lyrics = null
            lyricsPoll.start()
            return
        }
        lyricsPoll.stop()
        pane.lyrics = JSON.parse(raw)
        lyricsList.positionViewAtBeginning()
    }
    Timer { id: lyricsPoll; interval: 300; repeat: true; onTriggered: pane.loadLyrics() }
    onLyricsOnChanged: loadLyrics()
    Connections {
        target: Backend
        function onVideoIdChanged() { pane.loadLyrics() }
    }

    // Shape fallback (also visible while the cover loads).
    Item {
        anchors.fill: parent
        visible: !pane.hasArt
        Rectangle {
            x: parent.width - 182; y: 56
            width: 230; height: 230; radius: 115
            color: Theme.primary
        }
        Blob { x: -90; y: parent.height - 470; width: 380; height: 380; lobes: 12; amp: 3.2; base: 45; fillColor: Theme.secondaryContainer; spinning: pane.motion }
        Blob { x: 196; y: 228; width: 128; height: 128; lobes: 4; amp: 9; base: 40; fillColor: Theme.tertiary; spinning: pane.motion; reverse: true; period: 16000 }
        Blob { x: 40; y: 96; width: 64; height: 64; lobes: 8; amp: 5; base: 44; fillColor: Theme.onPrimaryContainer; spinning: pane.motion }
    }

    RoundImage {
        id: art
        anchors.fill: parent
        radius: pane.radius
        source: Backend.coverSource
    }

    // Lyrics: blurred cover (GPU only; software rendering gets a stronger
    // tint instead), dark tint, then the scrolling text.
    Item {
        anchors.fill: parent
        visible: pane.lyricsOn
        Image {
            id: blurSource
            anchors.fill: parent
            source: pane.lyricsOn ? Backend.coverSource : ""
            fillMode: Image.PreserveAspectCrop
            asynchronous: true
            visible: false
        }
        Rectangle {
            id: blurMask
            anchors.fill: parent
            radius: pane.radius
            layer.enabled: true
            visible: false
        }
        MultiEffect {
            id: blurFx
            anchors.fill: parent
            source: blurSource
            autoPaddingEnabled: false
            blurEnabled: true
            blur: 1.0
            blurMax: 64
            maskEnabled: true
            maskSource: blurMask
            maskThresholdMin: 0.5
            maskSpreadAtMin: 1.0
            visible: blurSource.status === Image.Ready && GraphicsInfo.api !== GraphicsInfo.Software
        }
        Rectangle {
            anchors.fill: parent
            radius: pane.radius
            color: Qt.rgba(0, 0, 0, blurFx.visible ? 0.55 : 0.85)
        }
        ListView {
            id: lyricsList
            anchors.fill: parent
            anchors.topMargin: 60
            anchors.bottomMargin: 12
            anchors.leftMargin: 28
            anchors.rightMargin: 28
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            spacing: 6
            model: pane.lyrics ? pane.lyrics.lines : []
            delegate: Text {
                required property string modelData
                width: lyricsList.width
                text: modelData.length > 0 ? modelData : " "
                color: Qt.rgba(1, 1, 1, 0.92)
                font.family: "Google Sans Flex"
                font.pixelSize: 22
                font.weight: Font.DemiBold
                wrapMode: Text.WordWrap
            }
            footer: Text {
                width: lyricsList.width
                topPadding: 20
                bottomPadding: 16
                visible: pane.lyrics !== null && pane.lyrics.source.length > 0
                text: pane.lyrics ? pane.lyrics.source : ""
                color: Qt.rgba(1, 1, 1, 0.6)
                font.family: "Roboto"
                font.pixelSize: 12
                wrapMode: Text.WordWrap
            }
        }
        Text {
            anchors.centerIn: parent
            width: parent.width - 56
            visible: pane.lyrics === null || pane.lyrics.lines.length === 0
            text: Backend.videoId.length === 0 ? qsTr("Play a song to see its lyrics")
                : pane.lyrics === null ? qsTr("Loading lyrics…") : qsTr("No lyrics for this song")
            color: Qt.rgba(1, 1, 1, 0.8)
            font.family: "Google Sans Flex"
            font.pixelSize: 18
            font.weight: Font.Medium
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
        }
    }

    // "Track N of M" chip (hidden until something is queued).
    Rectangle {
        x: 12; y: 12 // pane radius 28 - chip radius 16
        visible: Backend.queuePosition > 0
        height: 32
        width: chipRow.implicitWidth + 26
        radius: 16
        color: Theme.inverseSurface
        RowLayout {
            id: chipRow
            anchors.verticalCenter: parent.verticalCenter
            x: 12
            spacing: 8
            EqBars { barColor: Theme.inversePrimary; running: pane.motion }
            Text {
                text: qsTr("Track %1 of %2").arg(Backend.queuePosition).arg(Backend.queuePosition - 1 + Backend.queueCount)
                color: Theme.inverseOnSurface
                font.family: "Google Sans Flex"
                font.pixelSize: 13
                font.weight: Font.Medium
            }
        }
    }
}
