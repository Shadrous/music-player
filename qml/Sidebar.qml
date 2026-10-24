import QtQuick
import QtQuick.Layouts
import music_player

// 80px navigation rail: brand mark, destinations, account avatar. Layout
// only: emits navigate()/openLogin(); session state comes from Backend.
Item {
    id: rail
    property string activeTab: "home"
    signal navigate(string target)
    signal openLogin()

    implicitWidth: 80

    ColumnLayout {
        anchors.fill: parent
        anchors.topMargin: 8
        anchors.bottomMargin: 8
        spacing: 12

        Rectangle {
            Layout.alignment: Qt.AlignHCenter
            Layout.bottomMargin: 16
            width: 56; height: 56; radius: 16
            color: Theme.primaryContainer
            Icon {
                anchors.centerIn: parent
                name: "graphic_eq"
                filled: true
                size: 28
                color: Theme.onPrimaryContainer
            }
        }

        Repeater {
            model: [
                { id: "home", label: qsTr("Home"), icon: "home" },
                { id: "library", label: qsTr("Library"), icon: "library_music" },
                { id: "history", label: qsTr("History"), icon: "history" },
                { id: "settings", label: qsTr("Settings"), icon: "settings" }
            ]
            delegate: Item {
                id: dest
                required property var modelData
                readonly property bool on: rail.activeTab === modelData.id
                Layout.alignment: Qt.AlignHCenter
                implicitWidth: 72
                implicitHeight: 56

                Rectangle {
                    id: indicator
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: 56; height: 32; radius: 16
                    color: dest.on ? Theme.secondaryContainer : "transparent"
                    Behavior on color { ColorAnimation { duration: 200 } }
                    Rectangle {
                        anchors.fill: parent
                        radius: 16
                        color: Theme.onSurface
                        opacity: area.pressed ? 0.12 : area.containsMouse ? 0.08 : 0
                    }
                    Icon {
                        anchors.centerIn: parent
                        name: dest.modelData.icon
                        filled: dest.on
                        color: dest.on ? Theme.onSecondaryContainer : Theme.onSurfaceVariant
                    }
                }
                Text {
                    anchors.top: indicator.bottom
                    anchors.topMargin: 4
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: dest.modelData.label
                    color: dest.on ? Theme.onSurface : Theme.onSurfaceVariant
                    font.family: "Google Sans Flex"
                    font.pixelSize: 12
                    font.weight: dest.on ? Font.Bold : Font.Medium
                    font.letterSpacing: 0.5
                }
                MouseArea {
                    id: area
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        rail.activeTab = dest.modelData.id;
                        rail.navigate(dest.modelData.id);
                    }
                }
                Accessible.role: Accessible.PageTab
                Accessible.name: modelData.label
                Accessible.selected: on
                Accessible.onPressAction: area.clicked(null)
            }
        }

        Item { Layout.fillHeight: true }

        // Account avatar: initial when signed in, person glyph otherwise.
        Rectangle {
            id: avatar
            Layout.alignment: Qt.AlignHCenter
            width: 40; height: 40
            radius: avatarArea.pressed ? 8 : 20
            color: Backend.isLoggedIn ? Theme.tertiaryContainer : Theme.surfaceContainerHighest
            Behavior on radius { NumberAnimation { duration: 300; easing.type: Easing.OutBack } }
            Text {
                anchors.centerIn: parent
                visible: Backend.isLoggedIn && Backend.accountName.length > 0
                text: Backend.accountName.charAt(0).toUpperCase()
                color: Theme.onTertiaryContainer
                font.family: "Google Sans Flex"
                font.pixelSize: 16
                font.weight: Font.DemiBold
            }
            Icon {
                anchors.centerIn: parent
                visible: !Backend.sessionLoading && !(Backend.isLoggedIn && Backend.accountName.length > 0)
                name: "person"
                color: Backend.isLoggedIn ? Theme.onTertiaryContainer : Theme.onSurfaceVariant
            }
            Icon {
                anchors.centerIn: parent
                visible: Backend.sessionLoading
                name: "progress_activity"
                RotationAnimator on rotation {
                    from: 0; to: 360; duration: 1000
                    loops: Animation.Infinite
                    running: Backend.sessionLoading
                }
            }
            MouseArea {
                id: avatarArea
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: rail.openLogin()
            }
            Accessible.role: Accessible.Button
            Accessible.name: Backend.isLoggedIn ? qsTr("Account: ") + Backend.accountLabel : qsTr("Sign in")
            Accessible.onPressAction: rail.openLogin()
        }
    }
}
