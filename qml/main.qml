import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import spinwave

// Shell: 80px nav rail | 360px art pane | content column (search, the
// persistent Home/Library views + the queue panel, docked player).
// Navigation + dialog open/close only; data and theme live in Rust. Home and
// Library stay alive across navigation, so each keeps its models, scroll
// and cached rows. Narrow windows drop the art pane first, then let the
// queue panel take the whole content area; the minimum size keeps the
// essentials (nav, search, transport, queue).
ApplicationWindow {
    id: root
    visible: true
    width: 1280
    height: 800
    minimumWidth: 720
    minimumHeight: 560
    title: Backend.title.length > 0 ? Backend.title + " · " + qsTr("Spinwave") : qsTr("Spinwave")
    // Transparent so the rounded background below gives the frameless
    // window soft corners (the compositor only rounds decorated windows).
    color: "transparent"
    font.family: "Roboto"
    // The app draws its own title bar (window controls next to search).
    flags: Qt.Window | Qt.FramelessWindowHint
    readonly property bool maximized: visibility === Window.Maximized
    // Lyrics replace the cover art pane (mic button on the player).
    property bool lyricsOn: false
    // The art pane needs room beside the content; narrower windows drop it.
    readonly property bool coverShown: width >= 1180
    // Queue side panel (player bar's queue button).
    property bool queueOpen: false

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
    // OS media controls (media keys, GNOME/Windows panels) arrive on another
    // thread; apply them here.
    Timer {
        interval: 250
        repeat: true
        running: true
        onTriggered: {
            Backend.pollMediaKeys()
            Backend.pollActions()
        }
    }
    // Artist / album links anywhere open the page view; playlists open in Library.
    Connections {
        target: Backend
        function onPageChanged() {
            if (Backend.pageKind === "playlist") {
                contentHost.view = 1
                libraryView.openPlaylist(Backend.pageId, Backend.pageTitle, "")
                return
            }
            if (contentHost.view !== 4) contentHost.previousView = contentHost.view
            contentHost.view = 4
        }
    }
    // Playback URL/refill loads run on a worker; poll until the ticket lands.
    Timer {
        interval: 100
        repeat: true
        running: Backend.playbackState === "loading"
        onTriggered: Backend.pollPlayback()
    }

    // Packaged builds ship their fonts beside the QML (none on a dev checkout).
    Instantiator {
        model: JSON.parse(Backend.bundledFontsJson())
        delegate: FontLoader {
            required property string modelData
            source: modelData
        }
    }

    UpgradeDialog { id: upgradeDialog }
    CookieLoginDialog { id: cookieLogin }
    BrowserLoginDialog {
        id: browserLoginDialog
        onRequestCookieFallback: cookieLogin.open()
    }

    Rectangle {
        anchors.fill: parent
        // Concentric with the 28px panels 12px inside (28 + 12).
        radius: root.maximized ? 0 : 40
        color: Theme.surface
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
            visible: root.coverShown
            Layout.preferredWidth: 360
            Layout.fillHeight: true
            lyricsOn: root.lyricsOn
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

            RowLayout {
                id: contentRow
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 12
                // Too narrow for both: the queue takes the whole area.
                readonly property bool queueOnly: root.queueOpen && width < 720

                Item {
                    id: contentHost
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    visible: !contentRow.queueOnly
                    clip: true
                    // 0 home, 1 library, 2 history, 3 settings, 4 artist/album page.
                    property int view: 0
                    property int previousView: 0

                    HeroGrid {
                        id: home
                        anchors.fill: parent
                        visible: contentHost.view === 0
                    }
                    PageView {
                        anchors.fill: parent
                        visible: contentHost.view === 4
                        onBack: contentHost.view = contentHost.previousView
                    }
                    LibraryView {
                        id: libraryView
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

                QueuePanel {
                    visible: root.queueOpen
                    Layout.fillHeight: true
                    Layout.fillWidth: contentRow.queueOnly
                    Layout.preferredWidth: 340
                    onClose: root.queueOpen = false
                }
            }

            PlayerCard {
                Layout.fillWidth: true
                lyricsOn: root.lyricsOn && root.coverShown
                // Lyrics show in the art pane, so they go with it.
                lyricsAvailable: root.coverShown
                onToggleLyrics: root.lyricsOn = !root.lyricsOn
                queueOpen: root.queueOpen
                onToggleQueue: root.queueOpen = !root.queueOpen
                onRequestUpgrade: upgradeDialog.open()
            }
        }
    }

    // One-line messages above the player ("Song link copied", playlist
    // edits): M3 snackbar colors, gone after a few seconds.
    Rectangle {
        id: toast
        property bool shown: false
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 150
        width: Math.min(toastLabel.implicitWidth + 48, parent.width - 48)
        height: 48
        radius: 24
        color: Theme.inverseSurface
        opacity: shown ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { enabled: !Backend.lowPerformance; NumberAnimation { duration: 200 } }
        Text {
            id: toastLabel
            anchors.centerIn: parent
            width: Math.min(implicitWidth, toast.parent.width - 96)
            text: Backend.toastText
            color: Theme.inverseOnSurface
            font.family: "Roboto"
            font.pixelSize: 14
            elide: Text.ElideRight
        }
        Timer { id: toastTimer; interval: 2800; onTriggered: toast.shown = false }
        Connections {
            target: Backend
            function onToastChanged() {
                toast.shown = true
                toastTimer.restart()
            }
        }
        Accessible.role: Accessible.AlertMessage
        Accessible.name: Backend.toastText
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
