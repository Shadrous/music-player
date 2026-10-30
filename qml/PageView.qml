import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import spinwave

// Artist / album page (Backend.pageKind + pageId): cover, title, subtitle,
// Play / Shuffle, the songs, then card shelves (albums, singles, related
// artists; other versions of an album). Loads in the background and polls, like the
// other views. Layout only.
ScrollView {
    id: page
    clip: true
    contentWidth: availableWidth
    signal back()

    TrackModel { id: tracks }
    property var info: ({})
    property int trackCount: 0
    property bool loading: false
    property string status: ""
    property int token: 0
    property bool descriptionOpen: false
    readonly property bool isAlbum: Backend.pageKind === "album"

    function load() {
        // Playlists open in the Library view (main.qml routes them).
        if (Backend.pageId.length === 0 || Backend.pageKind === "playlist") return
        page.info = { title: Backend.pageTitle }
        tracks.clear()
        page.trackCount = 0
        page.loading = true
        page.status = ""
        page.descriptionOpen = false
        page.token = Backend.startPageLoad(Backend.gateStatus)
        poll.restart()
    }
    function play(index) {
        Backend.playQueue(tracks.rowsJson(), index)
        if (page.isAlbum)
            Backend.rememberCollection("album", page.info.id || Backend.pageId, page.info.title || "",
                page.info.artist || "", page.info.thumbnail_url || "")
    }
    function openCard(kind, title, id) {
        if (id.length > 0) Backend.openPage(kind, id, title)
    }

    Timer {
        id: poll
        interval: 100
        repeat: true
        onTriggered: {
            var state = Backend.pageLoadStatus(page.token)
            if (state === "loading") return
            stop()
            if (state === "stale") return
            page.loading = false
            if (state === "ready") {
                page.info = JSON.parse(Backend.pageJson())
                tracks.loadJson(JSON.stringify(page.info.tracks || []))
                page.trackCount = tracks.rowCount()
                if (page.trackCount === 0) page.status = qsTr("No songs here.")
            } else {
                page.status = state.substring(6)
            }
        }
    }
    Connections {
        target: Backend
        function onPageChanged() { page.load() }
    }
    Component.onCompleted: load()

    ColumnLayout {
        width: page.availableWidth
        spacing: 16

        PillButton {
            Layout.leftMargin: 8
            Layout.topMargin: 4
            implicitHeight: 32
            fontSize: 14
            icon: "arrow_back"
            // An album steps back to its artist (their other releases).
            readonly property bool toArtist: page.isAlbum && (page.info.artist_id || "").length > 0
            text: toArtist ? page.info.artist : qsTr("Back")
            onClicked: toArtist ? Backend.openPage("artist", page.info.artist_id, page.info.artist) : page.back()
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 8
            Layout.rightMargin: 8
            spacing: 20

            Rectangle {
                // Small windows keep the text, not the art.
                visible: page.availableWidth >= 480
                Layout.preferredWidth: 160
                Layout.preferredHeight: 160
                radius: page.isAlbum ? 28 : 80
                color: Theme.surfaceContainerHighest
                RoundImage {
                    anchors.fill: parent
                    radius: parent.radius
                    source: page.info.thumbnail_url || ""
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 8
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
                            text: page.isAlbum ? (page.info.release_type || qsTr("Album")) : qsTr("Artist")
                            color: Theme.onSecondaryContainer
                            font.family: "Google Sans Flex"
                            font.pixelSize: 14
                            font.weight: Font.Medium
                        }
                    }
                    BusyIndicator {
                        visible: page.loading
                        running: page.loading
                        Layout.preferredWidth: 20
                        Layout.preferredHeight: 20
                    }
                }
                Text {
                    Layout.fillWidth: true
                    text: page.info.title || ""
                    color: Theme.onSurface
                    font.family: "Google Sans Flex"
                    font.pixelSize: page.availableWidth >= 640 ? 56 : 36
                    font.weight: Font.Bold
                    font.letterSpacing: -1.2
                    elide: Text.ElideRight
                }
                Text {
                    Layout.fillWidth: true
                    visible: (page.info.artist || "").length > 0
                    text: page.info.artist || ""
                    color: Theme.onSurface
                    font.family: "Google Sans Flex"
                    font.pixelSize: 16
                    font.weight: Font.Medium
                    font.underline: (page.info.artist_id || "").length > 0 && artistArea.containsMouse
                    elide: Text.ElideRight
                    MouseArea {
                        id: artistArea
                        anchors.fill: parent
                        enabled: (page.info.artist_id || "").length > 0
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: Backend.openPage("artist", page.info.artist_id, page.info.artist)
                    }
                }
                Text {
                    Layout.fillWidth: true
                    visible: text.length > 0
                    text: page.status.length > 0 ? page.status : (page.info.subtitle || "")
                    color: Theme.onSurfaceVariant
                    font.family: "Roboto"
                    font.pixelSize: 14
                    elide: Text.ElideRight
                }
            }
        }

        Text {
            id: description
            Layout.fillWidth: true
            Layout.leftMargin: 8
            Layout.rightMargin: 8
            visible: (page.info.description || "").length > 0
            text: page.info.description || ""
            color: Theme.onSurfaceVariant
            font.family: "Roboto"
            font.pixelSize: 14
            lineHeight: 1.3
            wrapMode: Text.WordWrap
            maximumLineCount: page.descriptionOpen ? 1000 : 2
            elide: Text.ElideRight
            MouseArea {
                anchors.fill: parent
                enabled: description.truncated || page.descriptionOpen
                cursorShape: Qt.PointingHandCursor
                onClicked: page.descriptionOpen = !page.descriptionOpen
            }
        }

        RowLayout {
            Layout.leftMargin: 8
            spacing: 8
            PillButton {
                icon: "play_arrow"
                filledIcon: true
                text: qsTr("Play")
                bg: Theme.primary
                fg: Theme.onPrimary
                fontWeight: Font.DemiBold
                enabled: page.trackCount > 0 && Backend.playbackState !== "loading"
                onClicked: page.play(0)
            }
            PillButton {
                icon: "shuffle"
                text: qsTr("Shuffle")
                enabled: page.trackCount > 0
                onClicked: {
                    if (!Backend.shuffle) Backend.toggleShuffle()
                    page.play(Math.floor(Math.random() * page.trackCount))
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            visible: page.trackCount > 0
            implicitHeight: songs.implicitHeight + 16
            radius: 28
            color: Theme.surfaceContainerLow
            ColumnLayout {
                id: songs
                anchors.fill: parent
                anchors.margins: 8
                spacing: 2
                Text {
                    Layout.margins: 8
                    text: page.isAlbum ? qsTr("Songs") : qsTr("Top songs")
                    color: Theme.onSurface
                    font.family: "Google Sans Flex"
                    font.pixelSize: 22
                    font.weight: Font.DemiBold
                }
                Repeater {
                    model: tracks
                    delegate: TrackRow {
                        Layout.fillWidth: true
                        showNumber: page.isAlbum
                        playHandler: function() { page.play(index) }
                    }
                }
            }
        }

        Repeater {
            model: page.info.shelves || []
            delegate: CardGrid {
                id: shelf
                required property var modelData
                Layout.fillWidth: true
                title: modelData.title
                round: modelData.kind === "artist"
                model: round ? artists : albums
                onPicked: function(title, sub, id) { page.openCard(shelf.modelData.kind, title, id) }
                AlbumModel { id: albums }
                ArtistModel { id: artists }
                Component.onCompleted: (round ? artists : albums).loadJson(JSON.stringify(modelData.items))
            }
        }

        LegalLinks { Layout.leftMargin: 8; Layout.bottomMargin: 8 }
    }
}
