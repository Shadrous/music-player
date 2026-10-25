import QtQuick
import QtQuick.Layouts
import spinwave

// Art pane (360px, radius 28): the current cover, or the design's spinning
// M3 shapes when there is no art, with the queue-position chip. Layout only;
// the theme picker lives in SettingsView.
Rectangle {
    id: pane
    radius: 28
    clip: true
    color: Theme.primaryContainer

    readonly property bool hasArt: art.ready
    readonly property bool motion: Backend.isPlaying && !Backend.lowPerformance

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

    // "Track N of M" chip (hidden until something is queued).
    Rectangle {
        x: 16; y: 16
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
