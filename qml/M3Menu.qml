import QtQuick
import QtQuick.Controls
import spinwave

// M3 menu: rounded surface-container-high card with a soft shadow; submenus get
// the same look through `delegate`. Items are M3MenuItem.
Menu {
    id: menu
    padding: 8
    margins: 8
    implicitWidth: Math.max(264, contentWidth + leftPadding + rightPadding)
    delegate: M3MenuItem {}
    background: Rectangle {
        implicitWidth: 264
        radius: 16
        color: Theme.surfaceContainerHigh
        // Two-layer shadow: Qt Quick has no box-shadow without effects.
        Rectangle {
            z: -1
            anchors.fill: parent
            anchors.margins: -1
            anchors.topMargin: 1
            anchors.bottomMargin: -3
            radius: 17
            color: "#40000000"
        }
        Rectangle {
            z: -2
            anchors.fill: parent
            anchors.margins: -3
            anchors.topMargin: 2
            anchors.bottomMargin: -8
            radius: 20
            color: "#1f000000"
        }
    }
    enter: Transition {
        enabled: !Backend.lowPerformance
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 120 }
        NumberAnimation { property: "scale"; from: 0.94; to: 1; duration: 160; easing.type: Easing.OutCubic }
    }
    exit: Transition {
        enabled: !Backend.lowPerformance
        NumberAnimation { property: "opacity"; to: 0; duration: 90 }
    }
}
