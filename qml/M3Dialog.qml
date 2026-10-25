import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import spinwave

// M3 dialog shell: optional icon + headline, themed surface and scrim, no
// Fusion title bar or stock buttons. Content goes in `contentItem`; actions
// are PillButtons placed by the caller.
Dialog {
    id: dialog
    property string icon: ""
    property string headline: ""

    modal: true
    anchors.centerIn: Overlay.overlay
    padding: 24
    topPadding: 24
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

    header: ColumnLayout {
        spacing: 16
        visible: dialog.headline.length > 0
        Icon {
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: 24
            visible: dialog.icon.length > 0
            name: dialog.icon
            size: 28
            color: Theme.primary
        }
        Text {
            Layout.fillWidth: true
            Layout.topMargin: dialog.icon.length > 0 ? 0 : 24
            Layout.leftMargin: 24
            Layout.rightMargin: 24
            text: dialog.headline
            color: Theme.onSurface
            font.family: "Google Sans Flex"
            font.pixelSize: 24
            horizontalAlignment: dialog.icon.length > 0 ? Text.AlignHCenter : Text.AlignLeft
            wrapMode: Text.WordWrap
        }
    }
    background: Rectangle {
        radius: 28
        color: Theme.surfaceContainerHigh
    }
    Overlay.modal: Rectangle {
        // Same corners as the window background (main.qml), so the scrim
        // doesn't paint the transparent corners outside it.
        radius: Window.window && Window.window.visibility === Window.Maximized ? 0 : 40
        color: Qt.rgba(0, 0, 0, Theme.dark ? 0.6 : 0.32)
    }
    enter: Transition {
        enabled: !Backend.lowPerformance
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 150 }
        NumberAnimation { property: "scale"; from: 0.94; to: 1; duration: 250; easing.type: Easing.OutBack }
    }
    exit: Transition {
        enabled: !Backend.lowPerformance
        NumberAnimation { property: "opacity"; from: 1; to: 0; duration: 120 }
    }
}
