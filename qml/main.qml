import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import music_player

// Shell: 80px nav rail | 360px art pane | content column (search, the
// persistent Home/Library views, docked player). Navigation + dialog
// open/close only; data and theme live in Rust. Home and Library stay alive
// across navigation, so each keeps its models, scroll and cached rows.
ApplicationWindow {
    id: root
    visible: true
    width: 1280
    height: 800
    minimumWidth: 1100
    minimumHeight: 720
    title: Backend.title.length > 0 ? Backend.title + " · " + qsTr("Music Player") : qsTr("Music Player")
    color: Theme.surface
    font.family: "Roboto"
    // The app draws its own title bar (window controls next to search).
    flags: Qt.Window | Qt.FramelessWindowHint
    readonly property bool maximized: visibility === Window.Maximized

    // Fusion controls (dialogs, busy indicators) follow the M3 palette.
    palette.window: Theme.surfaceContainerHigh
    palette.windowText: Theme.onSurface
    palette.base: Theme.surfaceContainerHighest
    palette.alternateBase: Theme.surfaceContainer
    palette.text: Theme.onSurface
    palette.button: Theme.surfaceContainerHighest
    palette.buttonText: Theme.onSurface
    palette.highlight: Theme.primary
    palette.highlightedText: Theme.onPrimary
    palette.placeholderText: Theme.onSurfaceVariant
    palette.mid: Theme.outlineVariant
    palette.dark: Theme.outline

    // Launch restore runs on a Rust worker; poll until it lands.
    Component.onCompleted: {
        Backend.restoreSession()
        Backend.restorePlaybackSession()
    }
    onClosing: Backend.savePlaybackSession()
    Timer {
        interval: 100
        repeat: true
        running: Backend.sessionLoading
        onTriggered: Backend.pollSession()
    }
    // Playback URL/refill loads run on a worker; poll until the ticket lands.
    Timer {
        interval: 100
        repeat: true
        running: Backend.playbackState === "loading"
        onTriggered: Backend.pollPlayback()
    }

    UpgradeDialog { id: upgradeDialog }
    CookieLoginDialog { id: cookieLogin }
    BrowserLoginDialog {
        id: browserLoginDialog
        onRequestCookieFallback: cookieLogin.open()
    }

    // Empty space (margins, gaps, the art pane) moves the window; a double
    // click maximizes. Controls on top take their own clicks first.
    Item {
        anchors.fill: parent
        DragHandler {
            target: null
            onActiveChanged: if (active) root.startSystemMove()
        }
        TapHandler {
            onDoubleTapped: root.maximized ? root.showNormal() : root.showMaximized()
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.margins: 12
        spacing: 12

        Sidebar {
            id: rail
            Layout.fillHeight: true
            onNavigate: (target) => {
                contentHost.view = target === "library" ? 1 : target === "history" ? 2 : target === "settings" ? 3 : 0;
            }
            // The avatar opens the Account section of Settings.
            onOpenLogin: {
                rail.activeTab = "settings";
                contentHost.view = 3;
            }
        }

        CoverPanel {
            Layout.preferredWidth: 360
            Layout.fillHeight: true
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 12

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                // Search pill. Enter searches; the clear button returns home.
                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 56
                    radius: 28
                    color: Theme.surfaceContainerHigh
                    border.width: searchField.activeFocus ? 3 : 0
                    border.color: Theme.onSurfaceVariant
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 16
                        anchors.rightMargin: 8
                        spacing: 12
                        Icon { name: "search" }
                        TextField {
                            id: searchField
                            Layout.fillWidth: true
                            placeholderText: qsTr("Search songs, artists, albums")
                            placeholderTextColor: Theme.onSurfaceVariant
                            color: Theme.onSurface
                            font.family: "Roboto"
                            font.pixelSize: 16
                            background: null
                            selectByMouse: true
                            Accessible.name: placeholderText
                            onAccepted: {
                                Backend.setSearchQuery(text);
                                contentHost.view = 0;
                                rail.activeTab = "home";
                            }
                            // Follow queries started elsewhere (card / artist taps).
                            Connections {
                                target: Backend
                                function onSearchQueryChanged() { searchField.text = Backend.searchQuery }
                            }
                        }
                        IconButton {
                            visible: searchField.text.length > 0
                            icon: "close"
                            label: qsTr("Clear search")
                            onClicked: {
                                searchField.text = "";
                                Backend.setSearchQuery("");
                            }
                        }
                    }
                }
                PillButton {
                    visible: Backend.gateStatus === "upgradeRequired"
                    implicitHeight: 40
                    fontSize: 14
                    icon: "diamond"
                    filledIcon: true
                    text: qsTr("Upgrade")
                    bg: Theme.tertiaryContainer
                    fg: Theme.onTertiaryContainer
                    onClicked: upgradeDialog.open()
                }
                // Window controls, styled like the search pill.
                Rectangle {
                    Layout.preferredHeight: 56
                    implicitWidth: windowButtons.implicitWidth + 16
                    radius: 28
                    color: Theme.surfaceContainerHigh
                    Row {
                        id: windowButtons
                        anchors.centerIn: parent
                        spacing: 4
                        IconButton {
                            icon: "remove"
                            label: qsTr("Minimize")
                            onClicked: root.showMinimized()
                        }
                        IconButton {
                            icon: root.maximized ? "close_fullscreen" : "open_in_full"
                            iconSize: 20
                            label: root.maximized ? qsTr("Restore") : qsTr("Maximize")
                            onClicked: root.maximized ? root.showNormal() : root.showMaximized()
                        }
                        IconButton {
                            icon: "close"
                            label: qsTr("Close")
                            onClicked: root.close()
                        }
                    }
                }
            }

            Item {
                id: contentHost
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                property int view: 0

                HeroGrid {
                    id: home
                    anchors.fill: parent
                    visible: contentHost.view === 0
                }
                LibraryView {
                    anchors.fill: parent
                    visible: contentHost.view === 1 || contentHost.view === 2
                    section: contentHost.view === 2 ? "history" : "library"
                    activated: visible
                    onOpenLogin: browserLoginDialog.open()
                    onOpenUpgrade: upgradeDialog.open()
                }
                SettingsView {
                    anchors.fill: parent
                    visible: contentHost.view === 3
                    onOpenBrowserLogin: browserLoginDialog.open()
                    onOpenCookieLogin: cookieLogin.open()
                }
            }

            PlayerCard {
                Layout.fillWidth: true
                queueOpen: contentHost.view === 0 && home.showQueue
                onToggleQueue: {
                    home.showQueue = !queueOpen;
                    contentHost.view = 0;
                    rail.activeTab = "home";
                }
                onRequestUpgrade: upgradeDialog.open()
            }
        }
    }

    // Frameless resize: thin edge/corner grips hand off to the compositor.
    component Grip: Item {
        property int edges
        property int cursor
        HoverHandler { cursorShape: parent.cursor }
        DragHandler {
            target: null
            onActiveChanged: if (active) root.startSystemResize(parent.edges)
        }
    }
    Grip { visible: !root.maximized; edges: Qt.LeftEdge; cursor: Qt.SizeHorCursor; x: 0; y: 12; width: 6; height: parent.height - 24 }
    Grip { visible: !root.maximized; edges: Qt.RightEdge; cursor: Qt.SizeHorCursor; x: parent.width - 6; y: 12; width: 6; height: parent.height - 24 }
    Grip { visible: !root.maximized; edges: Qt.TopEdge; cursor: Qt.SizeVerCursor; x: 12; y: 0; width: parent.width - 24; height: 6 }
    Grip { visible: !root.maximized; edges: Qt.BottomEdge; cursor: Qt.SizeVerCursor; x: 12; y: parent.height - 6; width: parent.width - 24; height: 6 }
    Grip { visible: !root.maximized; edges: Qt.TopEdge | Qt.LeftEdge; cursor: Qt.SizeFDiagCursor; width: 12; height: 12 }
    Grip { visible: !root.maximized; edges: Qt.BottomEdge | Qt.RightEdge; cursor: Qt.SizeFDiagCursor; x: parent.width - 12; y: parent.height - 12; width: 12; height: 12 }
    Grip { visible: !root.maximized; edges: Qt.TopEdge | Qt.RightEdge; cursor: Qt.SizeBDiagCursor; x: parent.width - 12; width: 12; height: 12 }
    Grip { visible: !root.maximized; edges: Qt.BottomEdge | Qt.LeftEdge; cursor: Qt.SizeBDiagCursor; y: parent.height - 12; width: 12; height: 12 }
}
