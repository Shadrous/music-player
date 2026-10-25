import QtQuick
import QtQuick.Layouts
import spinwave

// Label + M3 switch (52x32 track; 24px handle with a check when on, 16px
// outline handle when off).
Item {
    id: toggle
    property string text
    property bool checked: false
    signal toggled()

    implicitWidth: row.implicitWidth
    implicitHeight: 32

    RowLayout {
        id: row
        anchors.fill: parent
        spacing: 12
        Text {
            text: toggle.text
            color: Theme.onSurface
            font.family: "Roboto"
            font.pixelSize: 14
        }
        Rectangle {
            width: 52; height: 32; radius: 16
            color: toggle.checked ? Theme.primary : Theme.surfaceContainerHighest
            border.width: 2
            border.color: toggle.checked ? Theme.primary : Theme.outline
            Behavior on color { enabled: !Backend.lowPerformance; ColorAnimation { duration: 200 } }
            Rectangle {
                width: toggle.checked ? 24 : 16
                height: width
                radius: width / 2
                anchors.verticalCenter: parent.verticalCenter
                x: toggle.checked ? parent.width - 28 : 6
                color: toggle.checked ? Theme.onPrimary : Theme.outline
                Behavior on x { enabled: !Backend.lowPerformance; NumberAnimation { duration: 300; easing.type: Easing.OutBack } }
                Behavior on width { enabled: !Backend.lowPerformance; NumberAnimation { duration: 300; easing.type: Easing.OutBack } }
                Icon {
                    anchors.centerIn: parent
                    visible: toggle.checked
                    name: "check"
                    size: 16
                    color: Theme.primary
                }
            }
        }
    }
    MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: toggle.toggled()
    }

    Accessible.role: Accessible.CheckBox
    Accessible.name: text
    Accessible.checked: checked
    Accessible.onToggleAction: toggle.toggled()
    Accessible.onPressAction: toggle.toggled()
}
