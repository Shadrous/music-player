import QtQuick
import spinwave

// Three bouncing equalizer bars (4px ↔ 16px), frozen when not `running`.
Row {
    id: eq
    property color barColor: Theme.primary
    property bool running: false
    spacing: 2
    height: 16

    Repeater {
        model: 3
        Rectangle {
            id: bar
            required property int index
            width: 3
            radius: 2
            anchors.bottom: parent.bottom
            color: eq.barColor
            height: 4 + 12 * (index === 1 ? 0.66 : index === 2 ? 0.33 : 1)
            SequentialAnimation on height {
                running: eq.running
                loops: Animation.Infinite
                PauseAnimation { duration: bar.index * 300 }
                NumberAnimation { to: 16; duration: 900; easing.type: Easing.OutCubic }
                NumberAnimation { to: 4; duration: 900; easing.type: Easing.OutCubic }
            }
        }
    }
}
