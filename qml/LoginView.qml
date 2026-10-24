import QtQuick
import QtQuick.Layouts
import music_player

// Account card body for Settings: avatar, name + Premium status, and the
// one relevant action (browser sign-in with a cookie fallback, or sign
// out). Layout only: sign-in, keyring, and the gate probe live in Rust.
ColumnLayout {
    id: login
    signal openBrowserLogin()
    signal openCookieLogin()

    readonly property bool premium: Backend.gateStatus === "premium"
    readonly property bool blocked: Backend.gateStatus === "upgradeRequired"
    spacing: 12

    RowLayout {
        Layout.fillWidth: true
        spacing: 16

        Rectangle {
            Layout.preferredWidth: 56
            Layout.preferredHeight: 56
            radius: 28
            color: Backend.isLoggedIn ? Theme.tertiaryContainer : Theme.surfaceContainerHighest
            Text {
                anchors.centerIn: parent
                visible: Backend.isLoggedIn && Backend.accountName.length > 0
                text: Backend.accountName.charAt(0).toUpperCase()
                color: Theme.onTertiaryContainer
                font.family: "Google Sans Flex"
                font.pixelSize: 22
                font.weight: Font.DemiBold
            }
            Icon {
                anchors.centerIn: parent
                visible: !(Backend.isLoggedIn && Backend.accountName.length > 0)
                name: Backend.sessionLoading ? "progress_activity" : "person"
                size: 28
                color: Theme.onSurfaceVariant
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2
            Text {
                Layout.fillWidth: true
                text: Backend.sessionLoading ? qsTr("Connecting…")
                    : Backend.isLoggedIn
                        ? (Backend.accountName.length > 0 ? Backend.accountName : qsTr("Signed in"))
                        : qsTr("Not signed in")
                color: Theme.onSurface
                font.family: "Google Sans Flex"
                font.pixelSize: 18
                font.weight: Font.Medium
                elide: Text.ElideRight
            }
            RowLayout {
                spacing: 6
                Icon {
                    visible: Backend.isLoggedIn
                    name: login.premium ? "verified" : "lock"
                    filled: true
                    size: 16
                    color: login.premium ? Theme.primary : Theme.tertiary
                }
                Text {
                    Layout.fillWidth: true
                    text: !Backend.isLoggedIn
                        ? qsTr("Sign in to sync your library and unlock playback.")
                        : login.premium ? qsTr("YouTube Music Premium · playback enabled")
                        : login.blocked ? qsTr("Playback locked — Premium could not be verified")
                        : qsTr("Session active")
                    color: Theme.onSurfaceVariant
                    font.family: "Roboto"
                    font.pixelSize: 13
                    wrapMode: Text.WordWrap
                }
            }
        }

        PillButton {
            visible: Backend.isLoggedIn
            implicitHeight: 40
            fontSize: 14
            icon: "logout"
            text: qsTr("Sign out")
            outlined: true
            fg: Theme.onSurfaceVariant
            onClicked: {
                var result = Backend.logout();
                statusLabel.text = result === "signed out" ? "" : result;
            }
        }
    }

    RowLayout {
        visible: !Backend.isLoggedIn && !Backend.sessionLoading
        spacing: 8
        PillButton {
            implicitHeight: 40
            fontSize: 14
            icon: "login"
            text: qsTr("Sign in with browser")
            bg: Theme.primary
            fg: Theme.onPrimary
            onClicked: login.openBrowserLogin()
        }
        PillButton {
            implicitHeight: 40
            fontSize: 14
            text: qsTr("Use a cookie instead")
            outlined: true
            fg: Theme.primary
            onClicked: login.openCookieLogin()
        }
    }
    Text {
        visible: !Backend.isLoggedIn && !Backend.sessionLoading
        Layout.fillWidth: true
        text: qsTr("A secure sign-in window opens; your password never touches this app. The session is kept in your system keyring, so you stay signed in.")
        color: Theme.onSurfaceVariant
        font.family: "Roboto"
        font.pixelSize: 12
        wrapMode: Text.WordWrap
    }
    Text {
        id: statusLabel
        Layout.fillWidth: true
        visible: text.length > 0
        color: Theme.tertiary
        font.family: "Roboto"
        font.pixelSize: 12
        wrapMode: Text.WordWrap
    }
    LegalLinks { Layout.fillWidth: true }
}
