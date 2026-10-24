import QtQuick
import music_player

// Round icon button with an M3 state layer. `toggle` buttons morph to a
// 12px-radius secondary-container square when `checked` (design `toggle()`);
// every button squeezes to radius 8 while pressed.
Rectangle {
    id: button
    property string icon
    property string label
    property bool filled: false
    property int iconSize: 24
    property bool toggle: false
    property bool checked: false
    property color fg: toggle && checked ? Theme.onSecondaryContainer : Theme.onSurfaceVariant
    property color bg: toggle && checked ? Theme.secondaryContainer : "transparent"
    property real restRadius: toggle && checked ? 12 : height / 2
    signal clicked()

    implicitWidth: 40
    implicitHeight: 40
    radius: area.pressed ? 8 : restRadius
    color: bg
    opacity: enabled ? 1 : 0.38
    Behavior on radius { NumberAnimation { duration: 300; easing.type: Easing.OutBack } }
    Behavior on color { ColorAnimation { duration: 200 } }

    Rectangle {
        anchors.fill: parent
        radius: parent.radius
        color: button.fg
        opacity: area.pressed ? 0.12 : area.containsMouse ? 0.08 : 0
        Behavior on opacity { NumberAnimation { duration: 200 } }
    }
    Icon {
        anchors.centerIn: parent
        name: button.icon
        size: button.iconSize
        filled: button.filled || (button.toggle && button.checked)
        color: button.fg
    }
    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: button.clicked()
    }

    Accessible.role: Accessible.Button
    Accessible.name: label
    Accessible.checkable: toggle
    Accessible.checked: checked
    Accessible.onPressAction: button.clicked()
}
