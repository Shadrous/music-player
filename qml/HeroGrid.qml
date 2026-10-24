import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import spinwave

// Home / search view: header (now playing or the search context), the song
// list with a Songs | Up next switch + autoplay, then album and artist
// cards. Layout only: rows come from Rust models, loads are background +
// polled.
ScrollView {
    id: browse
    clip: true
    contentWidth: availableWidth

    // Bound by the shell: the player bar's queue button flips this.
    property bool showQueue: false

    TrackModel { id: tracks }
    TrackModel { id: queue }
    AlbumModel { id: albums }
    ArtistModel { id: artists }

    // Rust-decided status line (live counts, empty states, failures).
    property string homeStatus: ""
    property bool loading: false
    property int loadToken: 0
    property int searchToken: 0
    // List models expose no `count`; refreshed after every fill/clear.
    property int trackCount: 0
    readonly property bool searching: Backend.searchQuery.trim().length > 0
    readonly property bool hasCurrent: Backend.videoId.length > 0

    // A gate/search change starts a BACKGROUND fetch (Rust thread) and polls
    // its status, so the window never freezes on the network.
    function scheduleHome() {
        searchPoll.stop()
        browse.loading = true
        // Launch restore still probing: its gateStatusChanged reloads us.
        if (Backend.sessionLoading) {
            loadPoll.stop()
            browse.homeStatus = qsTr("Connecting to your account…")
            return
        }
        browse.homeStatus = qsTr("Loading your music…")
        browse.loadToken = Backend.startHomeLoad(Backend.gateStatus)
        loadPoll.restart()
        browse.pollHome() // cached/fresh rows answer without waiting a tick
    }
    function scheduleSearch() {
        if (!browse.searching) {
            browse.scheduleHome()
            return
        }
        loadPoll.stop()
        browse.loading = true
        browse.homeStatus = qsTr("Searching…")
        browse.searchToken = Backend.startSearchLoad(Backend.searchQuery, Backend.gateStatus)
        searchPoll.restart()
    }
    function finish(state, fill, failText) {
        browse.loading = false
        if (state === "ready") {
            fill()
        } else if (state.indexOf("error:") === 0) {
            browse.homeStatus = state.substring(6)
            tracks.clear()
            albums.clear()
            artists.clear()
        } else {
            browse.homeStatus = failText
        }
        browse.trackCount = tracks.rowCount()
    }
    function pollSearch() {
        var state = Backend.searchLoadStatus(browse.searchToken)
        if (state === "loading") return
        searchPoll.stop()
        browse.finish(state, function() {
            browse.homeStatus = tracks.search(Backend.searchQuery, Backend.gateStatus)
            albums.search(Backend.searchQuery, Backend.gateStatus)
            artists.search(Backend.searchQuery, Backend.gateStatus)
        }, qsTr("Couldn't search. Try again."))
    }
    function pollHome() {
        var state = Backend.homeLoadStatus(browse.loadToken)
        if (state === "loading") return
        // "cached": saved rows show now (once); keep polling for the refresh.
        if (state === "cached" && !browse.loading) return
        if (state !== "cached") loadPoll.stop()
        browse.finish(state === "cached" ? "ready" : state, function() {
            browse.homeStatus = tracks.loadHome(Backend.gateStatus)
            albums.loadHome(Backend.gateStatus)
            artists.loadHome(Backend.gateStatus)
        }, qsTr("Couldn't load music. Try again."))
    }
    function refreshQueue() {
        if (browse.showQueue) queue.loadJson(Backend.queueRowsJson())
    }
    function playList(index) {
        Backend.playQueue(tracks.rowsJson(), index)
    }

    Timer { id: loadPoll; interval: 100; repeat: true; onTriggered: browse.pollHome() }
    Timer { id: searchPoll; interval: 100; repeat: true; onTriggered: browse.pollSearch() }

    Component.onCompleted: browse.scheduleHome()
    onShowQueueChanged: refreshQueue()
    Connections {
        target: Backend
        function onGateStatusChanged() { browse.scheduleSearch() }
        function onSearchQueryChanged() { browse.scheduleSearch() }
        function onSessionLoadingChanged() { if (Backend.sessionLoading) browse.scheduleHome() }
        function onVideoIdChanged() { browse.refreshQueue() }
        function onQueueCountChanged() { browse.refreshQueue() }
    }

    ColumnLayout {
        width: browse.availableWidth
        spacing: 16

        // ---- Header ----------------------------------------------------
        ColumnLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 8
            Layout.rightMargin: 8
            Layout.topMargin: 4
            spacing: 12

            RowLayout {
                spacing: 8
                Rectangle {
                    implicitHeight: 28
                    implicitWidth: kind.implicitWidth + 24
                    radius: 8
                    color: Theme.secondaryContainer
                    Text {
                        id: kind
                        anchors.centerIn: parent
                        text: browse.searching ? qsTr("Search") : browse.hasCurrent ? qsTr("Now playing") : qsTr("Home")
                        color: Theme.onSecondaryContainer
                        font.family: "Google Sans Flex"
                        font.pixelSize: 14
                        font.weight: Font.Medium
                    }
                }
                BusyIndicator {
                    visible: browse.loading
                    running: browse.loading
                    Layout.preferredWidth: 20
                    Layout.preferredHeight: 20
                }
                Text {
                    Layout.fillWidth: true
                    text: browse.homeStatus.length > 0 ? browse.homeStatus
                        : browse.trackCount > 0 ? qsTr("%1 songs").arg(browse.trackCount) : ""
                    color: Theme.onSurfaceVariant
                    font.family: "Google Sans Flex"
                    font.pixelSize: 14
                    font.weight: Font.Medium
                    elide: Text.ElideRight
                }
            }

            Text {
                Layout.fillWidth: true
                text: browse.searching ? "“" + Backend.searchQuery.trim() + "”"
                    : browse.hasCurrent ? Backend.title : qsTr("Home")
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
                Layout.fillWidth: true
                visible: browse.hasCurrent && Backend.artist.length > 0
                spacing: 10
                Rectangle {
                    width: 32; height: 32; radius: 16
                    color: Theme.tertiaryContainer
                    Text {
                        anchors.centerIn: parent
                        text: Backend.artist.charAt(0).toUpperCase()
                        color: Theme.onTertiaryContainer
                        font.family: "Google Sans Flex"
                        font.weight: Font.DemiBold
                        font.pixelSize: 14
                    }
                }
                Text {
                    text: Backend.artist
                    color: Theme.onSurface
                    font.family: "Google Sans Flex"
                    font.pixelSize: 16
                    font.weight: Font.Medium
                    elide: Text.ElideRight
                    Layout.maximumWidth: browse.availableWidth / 2
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: Backend.setSearchQuery(Backend.artist)
                    }
                }
                Text {
                    Layout.fillWidth: true
                    visible: Backend.nextTitle.length > 0
                    text: qsTr("Up next: ") + Backend.nextTitle
                    color: Theme.onSurfaceVariant
                    font.family: "Roboto"
                    font.pixelSize: 14
                    elide: Text.ElideRight
                }
            }

            RowLayout {
                Layout.topMargin: 4
                spacing: 8
                PillButton {
                    icon: Backend.isPlaying ? "pause" : "play_arrow"
                    filledIcon: true
                    text: Backend.isPlaying ? qsTr("Pause") : qsTr("Play")
                    bg: Theme.primary
                    fg: Theme.onPrimary
                    fontWeight: Font.DemiBold
                    enabled: Backend.playbackState !== "loading" && (browse.hasCurrent || browse.trackCount > 0)
                    onClicked: browse.hasCurrent ? Backend.togglePlay() : browse.playList(0)
                }
                PillButton {
                    icon: "shuffle"
                    text: qsTr("Shuffle")
                    enabled: browse.trackCount > 0
                    onClicked: {
                        if (!Backend.shuffle) Backend.toggleShuffle()
                        browse.playList(Math.floor(Math.random() * browse.trackCount))
                    }
                }
                PillButton {
                    icon: "sensors"
                    text: qsTr("Start radio")
                    outlined: !Backend.radioOn
                    fg: Backend.radioOn ? Theme.onSecondaryContainer : Theme.onSurfaceVariant
                    onClicked: Backend.toggleRadio()
                }
            }
        }

        // ---- Songs / Up next ------------------------------------------
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: songs.implicitHeight + 16
            radius: 28
            color: Theme.surfaceContainerLow

            ColumnLayout {
                id: songs
                anchors.fill: parent
                anchors.margins: 8
                spacing: 2

                RowLayout {
                    Layout.fillWidth: true
                    Layout.margins: 8
                    Layout.bottomMargin: 10
                    Segmented {
                        current: browse.showQueue ? 1 : 0
                        model: [
                            { icon: "album", text: qsTr("Songs") },
                            { icon: "queue_music", text: qsTr("Up next") }
                        ]
                        onPicked: (i) => browse.showQueue = i === 1
                    }
                    Item { Layout.fillWidth: true }
                    Toggle {
                        text: qsTr("Autoplay similar songs")
                        checked: Backend.autoplayOn
                        onToggled: Backend.toggleAutoplay()
                    }
                }

                Repeater {
                    id: songRows
                    model: browse.showQueue ? queue : tracks
                    delegate: TrackRow {
                        Layout.fillWidth: true
                        playHandler: browse.showQueue
                            ? function() { Backend.playQueueRow(video_id) }
                            : function() { browse.playList(index) }
                    }
                }

                // Up next lists only the next 100 (UP_NEXT_ROWS in Rust).
                Text {
                    Layout.fillWidth: true
                    Layout.margins: 16
                    visible: browse.showQueue && Backend.queueCount - 1 > songRows.count && songRows.count > 0
                    text: qsTr("+ %1 more in the queue").arg(Backend.queueCount - 1 - songRows.count)
                    color: Theme.onSurfaceVariant
                    font.family: "Roboto"
                    font.pixelSize: 14
                }
                Text {
                    Layout.fillWidth: true
                    Layout.margins: 16
                    visible: songRows.count === 0
                    text: browse.showQueue ? qsTr("Nothing up next. Play a song to start a queue.")
                        : browse.loading ? qsTr("Loading…") : qsTr("No songs here yet.")
                    color: Theme.onSurfaceVariant
                    font.family: "Roboto"
                    font.pixelSize: 14
                }
            }
        }

        // ---- Albums + artists -----------------------------------------
        CardGrid {
            Layout.fillWidth: true
            title: browse.searching ? qsTr("Albums") : qsTr("More to explore")
            model: albums
            onPicked: (title, sub) => Backend.setSearchQuery(title + " " + sub)
        }
        CardGrid {
            Layout.fillWidth: true
            title: qsTr("Artists")
            model: artists
            round: true
            onPicked: (title, sub) => Backend.setSearchQuery(title)
        }

        LegalLinks { Layout.leftMargin: 8; Layout.bottomMargin: 8 }
    }
}
