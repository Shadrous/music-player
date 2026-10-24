import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import music_player

// Upgrade surface. Layout only: copy comes from Rust
// (legal::upgrade_message + ToS link), link opens externally.
M3Dialog {
    id: upgrade
    icon: "diamond"
    headline: qsTr("YouTube Music Premium required")
    width: 460

    contentItem: ColumnLayout {
        spacing: 12

        Label {
            text: Backend.upgradeMessage()
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            font.pixelSize: 13
            color: Theme.onSurfaceVariant
        }
        LegalLinks {
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
        }
        Label {
            text: qsTr("Media is never downloaded or stored; ads are never skipped.")
            font.pixelSize: 12
            color: Theme.onSurfaceVariant
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
        }
        PillButton {
            Layout.alignment: Qt.AlignRight
            Layout.topMargin: 8
            implicitHeight: 40
            fontSize: 14
            text: qsTr("OK")
            bg: Theme.primary
            fg: Theme.onPrimary
            onClicked: upgrade.close()
        }
    }
}
