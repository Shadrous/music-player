import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import music_player

// Library surface: the account's playlists, playlist detail, and history.
// All loading is background + polled; rows are metadata-only. Rows are a
// virtualized ListView (only visible rows exist), and playlist detail loads
// page by page as the list nears its end.
Item {
    id: library

    signal openLogin()
    signal openUpgrade()

    readonly property bool isPremium: Backend.gateStatus === "premium"
    readonly property bool needsUpgrade: Backend.gateStatus === "upgradeRequired"

    property int libraryToken: 0
    property int playlistToken: 0
    // "library" (playlists, or one playlist's tracks) or "history".
    property string section: "library"
    // Loads only after the shell first navigates here; the instance then
    // stays alive, so returning is instant and keeps scroll/selection.
    property bool activated: false
    property string selectedPlaylistId: ""
    property string selectedPlaylistTitle: ""
    property string statusText: ""
    property bool loading: false
    // A next-page fetch is running (rows stay; the footer shows a spinner).
    property bool loadingMore: false
    readonly property bool inDetail: section === "library" && selectedPlaylistId.length > 0

    PlaylistModel { id: playlists }
    TrackModel { id: detail }
    TrackModel { id: history }

    function scheduleLibrary() {
        if (!library.activated) return
        if (!library.isPremium) {
            playlists.clear()
            history.clear()
            detail.clear()
            return
        }
        library.loading = true
        library.statusText = qsTr("Loading your library…")
        library.libraryToken = Backend.startLibraryLoad(Backend.gateStatus)
        libraryPoll.restart()
        library.pollLibrary() // cached/fresh rows answer without waiting a tick
    }
    function pollLibrary() {
        var state = Backend.libraryLoadStatus(library.libraryToken)
        if (state === "loading") return
        // "cached": saved rows show now (once); keep polling for the refresh.
        if (state === "cached" && !library.loading) return
        if (state !== "cached") libraryPoll.stop()
        library.loading = false
        if (state === "ready" || state === "cached") {
            playlists.loadJson(Backend.playlistsJson())
            history.loadJson(Backend.historyRowsJson())
            library.statusText = (playlists.rowCount() === 0 && history.rowCount() === 0)
                ? qsTr("No playlists or history yet.") : ""
        } else if (state.indexOf("error:") === 0) {
            library.statusText = state.substring(6)
        } else {
            library.statusText = qsTr("Couldn't load your library. Try again.")
        }
    }
    function countLabel(id, count) {
        if (id === "LM") return qsTr("Liked songs")
        return /[0-9]/.test(count) ? count + qsTr(" tracks") : qsTr("Playlist")
    }
    function openPlaylist(id, title) {
        library.selectedPlaylistId = id
        library.selectedPlaylistTitle = title
        library.loadingMore = false
        detail.clear()
        rowsView.positionViewAtBeginning()
        library.loading = true
        library.statusText = qsTr("Loading playlist…")
        library.playlistToken = Backend.startPlaylistLoad(id, Backend.gateStatus)
        playlistPoll.restart()
        library.pollPlaylist()
    }
    function closePlaylist() {
        playlistPoll.stop()
        library.selectedPlaylistId = ""
        library.selectedPlaylistTitle = ""
        library.loading = false
        library.loadingMore = false
        library.statusText = ""
        detail.clear()
    }
    function pollPlaylist() {
        var state = Backend.playlistLoadStatus(library.playlistToken)
        if (state === "loading") return
        if (library.loadingMore) {
            playlistPoll.stop()
            library.loadingMore = false
            if (state === "ready") {
                detail.appendJson(Backend.playlistRowsAfter(library.selectedPlaylistId, detail.rowCount()))
                Qt.callLater(library.maybeLoadMore)
            } else {
                library.statusText = state.indexOf("error:") === 0 ? state.substring(6) : qsTr("Couldn't load more of this playlist.")
            }
            return
        }
        if (state === "cached" && !library.loading) return
        if (state !== "cached") playlistPoll.stop()
        library.loading = false
        if (state === "ready" || state === "cached") {
            detail.loadJson(Backend.playlistRowsJson(library.selectedPlaylistId))
            library.statusText = ""
            Qt.callLater(library.maybeLoadMore) // a short first page may not fill the view
        } else {
            detail.clear()
            library.statusText = state.indexOf("error:") === 0 ? state.substring(6) : qsTr("Couldn't load this playlist.")
        }
    }
    // Near the end of a playlist: fetch the next page (one at a time, only
    // after the first page settled).
    function maybeLoadMore() {
        if (!library.inDetail || library.loading || library.loadingMore) return
        if (rowsView.contentHeight - rowsView.contentY - rowsView.height > 800) return
        if (!Backend.playlistHasMore(library.selectedPlaylistId)) return
        var token = Backend.startPlaylistMore(library.selectedPlaylistId, Backend.gateStatus)
        if (token <= 0) return
        library.loadingMore = true
        library.playlistToken = token
        playlistPoll.restart()
    }

    Timer { id: libraryPoll; interval: 100; repeat: true; onTriggered: library.pollLibrary() }
    Timer { id: playlistPoll; interval: 100; repeat: true; onTriggered: library.pollPlaylist() }

    Connections {
        target: Backend
        function onGateStatusChanged() { library.scheduleLibrary() }
    }
    // Local plays change while away, so refresh history on every visit.
    onVisibleChanged: if (visible && activated) history.loadJson(Backend.historyRowsJson())
    onActivatedChanged: if (activated) scheduleLibrary()

    // Signed-out / blocked empty state.
    ColumnLayout {
        x: (library.width - width) / 2
        y: Math.max(48, (library.height - height) / 2)
        width: Math.min(library.width - 96, 420)
        spacing: 16
        visible: !library.isPremium

        Rectangle {
            Layout.alignment: Qt.AlignHCenter
            width: 96; height: 96; radius: 28
            color: Theme.primaryContainer
            Icon {
                anchors.centerIn: parent
                name: library.section === "history" ? "history" : "library_music"
                size: 48
                filled: true
                color: Theme.onPrimaryContainer
            }
        }
        Text {
            Layout.fillWidth: true
            text: library.needsUpgrade ? qsTr("Premium required") : qsTr("Your library lives here")
            color: Theme.onSurface
            font.family: "Google Sans Flex"
            font.pixelSize: 28
            font.weight: Font.Bold
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
        }
        Text {
            Layout.fillWidth: true
            text: library.needsUpgrade ? Backend.upgradeMessage() : qsTr("Sign in to sync your playlists and history.")
            color: Theme.onSurfaceVariant
            font.family: "Roboto"
            font.pixelSize: 14
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
        }
        PillButton {
            Layout.alignment: Qt.AlignHCenter
            implicitHeight: 40
            fontSize: 14
            icon: library.needsUpgrade ? "diamond" : "login"
            filledIcon: true
            text: library.needsUpgrade ? qsTr("Upgrade") : qsTr("Sign in")
            bg: Theme.primary
            fg: Theme.onPrimary
            visible: library.needsUpgrade || !Backend.isLoggedIn
            onClicked: library.needsUpgrade ? library.openUpgrade() : library.openLogin()
        }
    }

    ListView {
        id: rowsView
        anchors.fill: parent
        visible: library.isPremium
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.vertical: ScrollBar {}
        model: library.inDetail ? detail : history
        onContentYChanged: library.maybeLoadMore()

        header: ColumnLayout {
            width: rowsView.width
            spacing: 16
                // Header.
                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.leftMargin: 8
                    Layout.rightMargin: 8
                    Layout.topMargin: 4
                    spacing: 12
                    RowLayout {
                        spacing: 8
                        PillButton {
                            visible: library.inDetail
                            implicitHeight: 32
                            fontSize: 14
                            icon: "arrow_back"
                            text: qsTr("Playlists")
                            onClicked: library.closePlaylist()
                        }
                        Rectangle {
                            visible: !library.inDetail
                            implicitHeight: 28
                            implicitWidth: kind.implicitWidth + 24
                            radius: 8
                            color: Theme.secondaryContainer
                            Text {
                                id: kind
                                anchors.centerIn: parent
                                text: library.section === "history" ? qsTr("History") : qsTr("Library")
                                color: Theme.onSecondaryContainer
                                font.family: "Google Sans Flex"
                                font.pixelSize: 14
                                font.weight: Font.Medium
                            }
                        }
                        BusyIndicator {
                            visible: library.loading
                            running: library.loading
                            Layout.preferredWidth: 20
                            Layout.preferredHeight: 20
                        }
                        Text {
                            text: library.statusText.length > 0 ? library.statusText : Backend.accountLabel
                            color: Theme.onSurfaceVariant
                            font.family: "Google Sans Flex"
                            font.pixelSize: 14
                            font.weight: Font.Medium
                        }
                    }
                    Text {
                        Layout.fillWidth: true
                        text: library.section === "history" ? qsTr("Recently played")
                            : library.inDetail ? library.selectedPlaylistTitle : qsTr("Your library")
                        color: Theme.onSurface
                        font.family: "Google Sans Flex"
                        font.pixelSize: 64
                        font.weight: Font.Bold
                        font.letterSpacing: -1.5
                        lineHeight: 68
                        lineHeightMode: Text.FixedHeight
                        elide: Text.ElideRight
                    }
                    RowLayout {
                        visible: library.section === "history" || library.inDetail
                        spacing: 8
                        PillButton {
                            icon: "play_arrow"
                            filledIcon: true
                            text: qsTr("Play")
                            bg: Theme.primary
                            fg: Theme.onPrimary
                            fontWeight: Font.DemiBold
                            enabled: rowsView.count > 0 && Backend.playbackState !== "loading"
                            onClicked: Backend.playQueue(rowsView.model.rowsJson(), 0)
                        }
                        PillButton {
                            icon: "shuffle"
                            text: qsTr("Shuffle")
                            enabled: rowsView.count > 0
                            onClicked: {
                                if (!Backend.shuffle) Backend.toggleShuffle()
                                Backend.playQueue(rowsView.model.rowsJson(), Math.floor(Math.random() * rowsView.count))
                            }
                        }
                    }
                }

                // Playlist cards (library root only).
                GridLayout {
                    Layout.fillWidth: true
                    Layout.leftMargin: 4
                    Layout.rightMargin: 4
                    visible: library.section === "library" && !library.inDetail
                    columns: Math.max(1, Math.floor((width + 8) / 248))
                    columnSpacing: 8
                    rowSpacing: 8
                    Repeater {
                        model: playlists
                        delegate: Rectangle {
                            id: card
                            required property string title
                            required property string playlist_id
                            required property string count
                            required property string thumbnail_url
                            Layout.fillWidth: true
                            Layout.preferredWidth: 240
                            implicitHeight: 84
                            radius: 20
                            color: Theme.surfaceContainer
                            Rectangle {
                                anchors.fill: parent
                                radius: parent.radius
                                color: Theme.onSurface
                                opacity: area.pressed ? 0.12 : area.containsMouse ? 0.08 : 0
                            }
                            RowLayout {
                                anchors.fill: parent
                                anchors.margins: 10
                                spacing: 12
                                Rectangle {
                                    Layout.preferredWidth: 64
                                    Layout.preferredHeight: 64
                                    radius: 16
                                    color: Theme.secondaryContainer
                                    Icon {
                                        anchors.centerIn: parent
                                        visible: !art.ready
                                        name: card.playlist_id === "LM" ? "favorite" : "queue_music"
                                        filled: true
                                        color: Theme.onSecondaryContainer
                                    }
                                    RoundImage {
                                        id: art
                                        anchors.fill: parent
                                        radius: 16
                                        source: card.thumbnail_url
                                    }
                                }
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 0
                                    Text {
                                        Layout.fillWidth: true
                                        text: card.title
                                        color: Theme.onSurface
                                        font.family: "Google Sans Flex"
                                        font.pixelSize: 16
                                        font.weight: Font.Medium
                                        elide: Text.ElideRight
                                    }
                                    Text {
                                        Layout.fillWidth: true
                                        text: library.countLabel(card.playlist_id, card.count)
                                        color: Theme.onSurfaceVariant
                                        font.family: "Roboto"
                                        font.pixelSize: 12
                                        elide: Text.ElideRight
                                    }
                                }
                            }
                            MouseArea {
                                id: area
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: library.openPlaylist(card.playlist_id, card.title)
                            }
                            Accessible.role: Accessible.Button
                            Accessible.name: card.title
                            Accessible.onPressAction: library.openPlaylist(card.playlist_id, card.title)
                        }
                    }
                }


                // Rounded top of the rows card.
                Item {
                    Layout.fillWidth: true
                    visible: rowsView.count > 0
                    implicitHeight: recent.visible ? recent.implicitHeight + 24 : 8
                    clip: true
                    Rectangle {
                        width: parent.width
                        height: parent.height + 28
                        radius: 28
                        color: Theme.surfaceContainerLow
                    }
                    Text {
                        id: recent
                        x: 16; y: 16
                        visible: library.section === "library" && !library.inDetail
                        text: qsTr("Recently played")
                        color: Theme.onSurface
                        font.family: "Google Sans Flex"
                        font.pixelSize: 22
                        font.weight: Font.DemiBold
                    }
                }
            }

        // Tracks: playlist detail, history, or "recently played" under the cards.
        delegate: Rectangle {
            id: slot
            required property int index
            required property string title
            required property string artist
            required property string album
            required property string video_id
            required property string duration
            required property string thumbnail_url
            width: rowsView.width
            height: 58
            color: Theme.surfaceContainerLow
            TrackRow {
                x: 8
                width: parent.width - 16
                height: 56
                index: slot.index
                title: slot.title
                artist: slot.artist
                album: slot.album
                video_id: slot.video_id
                duration: slot.duration
                thumbnail_url: slot.thumbnail_url
                // shortcut: Play/Shuffle queue the pages loaded so far, not the whole playlist; fetch the rest first if users ask.
                playHandler: function() { Backend.playQueue(rowsView.model.rowsJson(), slot.index) }
            }
        }

        footer: ColumnLayout {
            width: rowsView.width
            spacing: 16
            // Rounded bottom of the rows card (+ next-page spinner).
            Item {
                Layout.fillWidth: true
                visible: rowsView.count > 0
                implicitHeight: library.loadingMore ? 56 : 8
                clip: true
                Rectangle {
                    y: -28
                    width: parent.width
                    height: parent.height + 28
                    radius: 28
                    color: Theme.surfaceContainerLow
                }
                BusyIndicator {
                    anchors.centerIn: parent
                    visible: library.loadingMore
                    running: library.loadingMore
                    width: 28; height: 28
                }
            }
            LegalLinks { Layout.leftMargin: 8; Layout.bottomMargin: 8 }
        }
    }
}
