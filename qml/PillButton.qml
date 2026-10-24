import QtQuick
import QtQuick.Layouts
import music_player

// Icon + label pill (Play / Shuffle / Start radio / Upgrade). `outlined`
// draws the outline-variant border on a transparent fill.
Rectangle {
    id: pill
    property string icon
    property string text
    property bool filledIcon: false
    property color fg: Theme.onSecondaryContainer
    property color bg: Theme.secondaryContainer
    property bool outlined: false
    property int fontSize: 16
    property int fontWeight: Font.Medium
    signal clicked()

    implicitHeight: 56
    implicitWidth: row.implicitWidth + (implicitHeight >= 56 ? 46 : 28)
    radius: area.pressed ? 8 : height / 2
    color: outlined ? "transparent" : bg
    border.width: outlined ? 1 : 0
    border.color: Theme.outlineVariant
    opacity: enabled ? 1 : 0.38
    Behavior on radius { NumberAnimation { duration: 300; easing.type: Easing.OutBack } }
    Behavior on color { ColorAnimation { duration: 200 } }

    Rectangle {
        anchors.fill: parent
        radius: parent.radius
        color: pill.fg
        opacity: area.pressed ? 0.12 : area.containsMouse ? 0.08 : 0
        Behavior on opacity { NumberAnimation { duration: 200 } }
    }
    RowLayout {
        id: row
        anchors.centerIn: parent
        anchors.horizontalCenterOffset: -2
        spacing: 8
        Icon {
            name: pill.icon
            size: pill.implicitHeight >= 56 ? 24 : 18
            filled: pill.filledIcon
            color: pill.fg
            visible: pill.icon.length > 0
        }
        Text {
            text: pill.text
            color: pill.fg
            font.family: "Google Sans Flex"
            font.pixelSize: pill.fontSize
            font.weight: pill.fontWeight
        }
    }
    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: pill.clicked()
    }

    Accessible.role: Accessible.Button
    Accessible.name: text
    Accessible.onPressAction: pill.clicked()
}
