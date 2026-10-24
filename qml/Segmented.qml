import QtQuick
import QtQuick.Layouts
import spinwave

// Outlined M3 segmented buttons. `model`: [{ icon, label, text? }]. With
// `text`, the selected segment swaps its icon for a check (design).
Item {
    id: seg
    property var model: []
    property int current: 0
    property int segmentHeight: 40
    property int segmentWidth: 0 // 0 = fit content
    signal picked(int index)

    implicitHeight: segmentHeight
    implicitWidth: row.implicitWidth + 2

    // End segments round their outer side only (`clip` can't follow a radius).
    component SegmentShape: Rectangle {
        property bool first: false
        property bool last: false
        radius: first || last ? height / 2 : 0
        Rectangle {
            visible: parent.radius > 0 && !(parent.first && parent.last)
            width: parent.width / 2
            height: parent.height
            x: parent.first ? parent.width / 2 : 0
            color: parent.color
        }
    }

    RowLayout {
        id: row
        anchors.fill: parent
        anchors.margins: 1
        spacing: 0
        Repeater {
            model: seg.model
            delegate: Item {
                id: item
                required property var modelData
                required property int index
                readonly property bool on: seg.current === index
                readonly property bool hasText: (modelData.text || "").length > 0
                readonly property bool first: index === 0
                readonly property bool last: index === seg.model.length - 1
                Layout.fillHeight: true
                implicitWidth: seg.segmentWidth > 0 ? seg.segmentWidth : content.implicitWidth + 28

                SegmentShape {
                    anchors.fill: parent
                    first: item.first
                    last: item.last
                    color: item.on ? Theme.secondaryContainer : "transparent"
                }
                SegmentShape {
                    anchors.fill: parent
                    first: item.first
                    last: item.last
                    color: item.on ? Theme.onSecondaryContainer : Theme.onSurface
                    opacity: area.pressed ? 0.12 : area.containsMouse ? 0.08 : 0
                }
                Rectangle {
                    visible: !item.first
                    width: 1
                    height: parent.height
                    color: Theme.outline
                }
                RowLayout {
                    id: content
                    anchors.centerIn: parent
                    spacing: 6
                    Icon {
                        name: item.hasText && item.on ? "check" : item.modelData.icon
                        filled: item.on
                        size: 18
                        color: item.on ? Theme.onSecondaryContainer : (item.hasText ? Theme.onSurface : Theme.onSurfaceVariant)
                    }
                    Text {
                        visible: item.hasText
                        text: item.modelData.text || ""
                        color: item.on ? Theme.onSecondaryContainer : Theme.onSurface
                        font.family: "Google Sans Flex"
                        font.pixelSize: 14
                        font.weight: Font.Medium
                    }
                }
                MouseArea {
                    id: area
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: seg.picked(item.index)
                }
                Accessible.role: Accessible.Button
                Accessible.name: item.modelData.label || item.modelData.text
                Accessible.checkable: true
                Accessible.checked: item.on
                Accessible.onPressAction: seg.picked(item.index)
            }
        }
    }

    Rectangle {
        anchors.fill: parent
        radius: height / 2
        color: "transparent"
        border.width: 1
        border.color: Theme.outline
    }
}
