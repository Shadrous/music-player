import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import spinwave

// Fallback sign-in for environments where the embedded browser cannot open
// (or when the user prefers their existing browser session). Accepts the same
// Cookie request-header value the browser sends to music.youtube.com; Rust
// validates, stores it in the OS keyring, and runs the live gate probe.
M3Dialog {
    id: cookieDialog

    signal connected()

    icon: "cookie"
    headline: qsTr("Connect with a browser cookie")
    width: 560
    property string statusText: ""



    onOpened: {
        cookieField.text = "";
        cookieDialog.statusText = "";
    }

    contentItem: ColumnLayout {
        spacing: 12

        Label {
            Layout.fillWidth: true
            text: qsTr("Reuse the YouTube Music session from your browser. No password is entered here, and the cookie is stored only in your OS keyring.")
            color: Theme.onSurfaceVariant
            font.pixelSize: 13
            wrapMode: Text.WordWrap
        }

        PillButton {
            implicitHeight: 40
            fontSize: 14
            icon: "open_in_new"
            text: qsTr("Open music.youtube.com")
            outlined: true
            fg: Theme.primary
            onClicked: Qt.openUrlExternally(Backend.ytmusicUrl())
        }

        Label {
            Layout.fillWidth: true
            text: qsTr("1. Sign in to YouTube Music in your browser.\n2. Open developer tools (F12) and select the Network tab.\n3. Reload the page, choose any request to music.youtube.com, and copy the value of its Cookie request header.")
            color: Theme.onSurfaceVariant
            font.pixelSize: 12
            wrapMode: Text.WordWrap
            lineHeight: 1.25
        }

        TextArea {
            id: cookieField
            Layout.fillWidth: true
            Layout.preferredHeight: 130
            placeholderText: qsTr("Paste the Cookie request-header value here")
            placeholderTextColor: Theme.onSurfaceVariant
            color: Theme.onSurface
            wrapMode: Text.WrapAnywhere
            selectByMouse: true
            font.family: "Roboto"
            font.pixelSize: 13
            padding: 12
            background: Rectangle {
                radius: 12
                color: Theme.surfaceContainerHighest
                border.width: cookieField.activeFocus ? 2 : 1
                border.color: cookieField.activeFocus ? Theme.primary : Theme.outline
            }
        }

        Label {
            Layout.fillWidth: true
            text: qsTr("The header must include __Secure-3PAPISID for YouTube to accept the session.")
            font.pixelSize: 11
            color: Theme.onSurfaceVariant
            wrapMode: Text.WordWrap
        }

        Label {
            Layout.fillWidth: true
            text: cookieDialog.statusText
            visible: text.length > 0
            color: Theme.tertiary
            font.pixelSize: 12
            wrapMode: Text.WordWrap
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.topMargin: 8
            spacing: 8
            Item { Layout.fillWidth: true }
            PillButton {
                implicitHeight: 40
                fontSize: 14
                text: qsTr("Cancel")
                bg: "transparent"
                fg: Theme.primary
                onClicked: cookieDialog.close()
            }
            PillButton {
                implicitHeight: 40
                fontSize: 14
                bg: Theme.primary
                fg: Theme.onPrimary
                text: qsTr("Connect")
                enabled: cookieField.text.trim().length > 0
                onClicked: {
                    var result = Backend.connectWithCookie(cookieField.text);
                    if (result === "ok") {
                        cookieDialog.connected();
                        cookieDialog.close();
                    } else {
                        cookieDialog.statusText = result;
                    }
                }
            }
        }
    }
}
