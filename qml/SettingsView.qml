import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import spinwave

// Settings: account (sign-in state + actions), playback resume, and
// appearance (light/dark + color seed, saved by the Rust Theme singleton).
// Layout only.
ScrollView {
    id: settings
    clip: true
    contentWidth: availableWidth

    signal openBrowserLogin()
    signal openCookieLogin()

    component SectionTitle: Text {
        Layout.leftMargin: 8
        Layout.topMargin: 8
        color: Theme.onSurface
        font.family: "Google Sans Flex"
        font.pixelSize: 22
        font.weight: Font.DemiBold
    }

    // Title + hint on the left, switch on the right.
    component SettingRow: RowLayout {
        id: setting
        property string title
        property string hint
        property bool checked
        signal toggled()
        Layout.fillWidth: true
        spacing: 16
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2
            Text {
                text: setting.title
                color: Theme.onSurface
                font.family: "Google Sans Flex"
                font.pixelSize: 18
                font.weight: Font.Medium
            }
            Text {
                Layout.fillWidth: true
                text: setting.hint
                color: Theme.onSurfaceVariant
                font.family: "Roboto"
                font.pixelSize: 13
                wrapMode: Text.WordWrap
            }
        }
        Toggle {
            checked: setting.checked
            enabled: setting.enabled
            onToggled: setting.toggled()
            Accessible.name: setting.title
        }
    }

    readonly property var seedNames: ({
        mono: qsTr("Mono"), ember: qsTr("Ember"), ocean: qsTr("Ocean"),
        forest: qsTr("Forest"), orchid: qsTr("Orchid")
    })

    ColumnLayout {
        width: settings.availableWidth
        spacing: 16

        ColumnLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 8
            Layout.rightMargin: 8
            Layout.topMargin: 4
            spacing: 12
            Rectangle {
                implicitHeight: 28
                implicitWidth: kind.implicitWidth + 24
                radius: 8
                color: Theme.secondaryContainer
                Text {
                    id: kind
                    anchors.centerIn: parent
                    text: qsTr("Preferences")
                    color: Theme.onSecondaryContainer
                    font.family: "Google Sans Flex"
                    font.pixelSize: 14
                    font.weight: Font.Medium
                }
            }
            Text {
                text: qsTr("Settings")
                color: Theme.onSurface
                font.family: "Google Sans Flex"
                font.pixelSize: 64
                font.weight: Font.Bold
                font.letterSpacing: -1.5
            }
        }

        SectionTitle { text: qsTr("Account") }
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: account.implicitHeight + 48
            radius: 28
            color: Theme.surfaceContainerLow
            LoginView {
                id: account
                anchors.fill: parent
                anchors.margins: 24
                onOpenBrowserLogin: settings.openBrowserLogin()
                onOpenCookieLogin: settings.openCookieLogin()
            }
        }

        SectionTitle { text: qsTr("Playback") }
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: resume.implicitHeight + 48
            radius: 28
            color: Theme.surfaceContainerLow
            ColumnLayout {
                id: resume
                anchors.fill: parent
                anchors.margins: 24
                spacing: 24
                SettingRow {
                    title: qsTr("Resume where you left off")
                    hint: qsTr("Reopen your last queue and song when the app starts")
                    checked: Backend.resumeEnabled
                    onToggled: Backend.setResumeEnabled(!Backend.resumeEnabled)
                }
                Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: Theme.outlineVariant }
                SettingRow {
                    enabled: Backend.resumeEnabled
                    opacity: enabled ? 1 : 0.38
                    title: qsTr("Resume at the exact moment")
                    hint: qsTr("Off: the song starts again from the beginning")
                    checked: Backend.resumePosition
                    onToggled: Backend.setResumePosition(!Backend.resumePosition)
                }
                Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: Theme.outlineVariant }
                SettingRow {
                    title: qsTr("Low-performance mode")
                    hint: qsTr("For older or busy computers: turns off animations and next-song preloading, and draws without the graphics card (restart to apply that part)")
                    checked: Backend.lowPerformance
                    onToggled: Backend.setLowPerformance(!Backend.lowPerformance)
                }
            }
        }

        SectionTitle { text: qsTr("Appearance") }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: body.implicitHeight + 48
            radius: 28
            color: Theme.surfaceContainerLow

            ColumnLayout {
                id: body
                anchors.fill: parent
                anchors.margins: 24
                spacing: 24

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 16
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2
                        Text {
                            text: qsTr("Theme")
                            color: Theme.onSurface
                            font.family: "Google Sans Flex"
                            font.pixelSize: 18
                            font.weight: Font.Medium
                        }
                        Text {
                            text: qsTr("Light or dark surfaces")
                            color: Theme.onSurfaceVariant
                            font.family: "Roboto"
                            font.pixelSize: 13
                        }
                    }
                    Segmented {
                        current: Theme.dark ? 1 : 0
                        model: [
                            { icon: "light_mode", text: qsTr("Light") },
                            { icon: "dark_mode", text: qsTr("Dark") }
                        ]
                        onPicked: (i) => Theme.setDark(i === 1)
                    }
                }

                Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: Theme.outlineVariant }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 16
                    ColumnLayout {
                        spacing: 2
                        Text {
                            text: qsTr("Color")
                            color: Theme.onSurface
                            font.family: "Google Sans Flex"
                            font.pixelSize: 18
                            font.weight: Font.Medium
                        }
                        Text {
                            text: qsTr("Mono is plain black and white; the others tint the whole player")
                            color: Theme.onSurfaceVariant
                            font.family: "Roboto"
                            font.pixelSize: 13
                        }
                    }
                    Flow {
                        Layout.fillWidth: true
                        spacing: 16
                        Repeater {
                            model: JSON.parse(Theme.seedsJson())
                            delegate: Column {
                                id: swatch
                                required property var modelData
                                readonly property bool on: Theme.seed === modelData.id
                                spacing: 8
                                Rectangle {
                                    anchors.horizontalCenter: parent.horizontalCenter
                                    width: 56; height: 56
                                    radius: area.pressed ? 8 : swatch.on ? 18 : 28
                                    color: swatch.modelData.hex
                                    border.width: swatch.on ? 3 : 0
                                    border.color: Theme.onSurface
                                    Behavior on radius { enabled: !Backend.lowPerformance; NumberAnimation { duration: 300; easing.type: Easing.OutBack } }
                                    Icon {
                                        anchors.centerIn: parent
                                        visible: swatch.on
                                        name: "check"
                                        filled: true
                                        size: 22
                                        color: "#ffffff"
                                    }
                                    MouseArea {
                                        id: area
                                        anchors.fill: parent
                                        cursorShape: Qt.PointingHandCursor
                                        onClicked: Theme.setSeed(swatch.modelData.id)
                                    }
                                }
                                Text {
                                    anchors.horizontalCenter: parent.horizontalCenter
                                    text: settings.seedNames[swatch.modelData.id] || swatch.modelData.id
                                    color: swatch.on ? Theme.onSurface : Theme.onSurfaceVariant
                                    font.family: "Google Sans Flex"
                                    font.pixelSize: 13
                                    font.weight: swatch.on ? Font.DemiBold : Font.Normal
                                }
                                Accessible.role: Accessible.RadioButton
                                Accessible.name: settings.seedNames[modelData.id] || modelData.id
                                Accessible.checked: on
                                Accessible.onPressAction: Theme.setSeed(modelData.id)
                            }
                        }
                    }
                }
            }
        }
    }
}
