import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import spinwave

// Song row (min 56px): small cover (or track number), title + artist/album
// links, hover actions (like, dislike, add to queue, more menu), duration.
// The current track (matched by video id) sits on a secondary-container
// pill. Dragging a row sideways carries it to the queue panel. Layout and
// one-line actions only: `playHandler` is the list owner's play action.
Item {
    id: row
    required property string title
    required property string artist
    required property string album
    required property string video_id
    required property string duration
    required property string thumbnail_url
    required property string artist_id
    required property string album_id
    required property string set_video_id
    // Qt6 exposes the view's `index` to delegates with required props only
    // through a matching required property.
    required property int index
    property int rowIndex: index
    property var playHandler: null
    // Album pages: number the rows (their tracks share one cover).
    property bool showNumber: false
    // Queue panel rows reorder through their own handle instead.
    property bool draggable: true
    // Narrow rows keep only the more button; the menu takes the rest.
    readonly property bool compact: width < 560

    readonly property bool isCurrent: Backend.videoId.length > 0 && Backend.videoId === video_id
    readonly property color subFg: isCurrent ? Theme.onSecondaryContainer : Theme.onSurfaceVariant
    // `ratingsVersion` makes the binding re-read after any rating change.
    readonly property string rating: Backend.ratingsVersion >= 0 ? Backend.ratingOf(video_id) : ""
    readonly property bool showActions: hover.hovered || menuLoader.active

    implicitHeight: 56

    // Flat row for the queue / drag payload (same shape the models read).
    function rowJson() {
        return JSON.stringify({
            video_id: row.video_id, title: row.title, artist: row.artist, album: row.album,
            duration: row.duration, thumbnail_url: row.thumbnail_url,
            artist_id: row.artist_id, album_id: row.album_id, set_video_id: row.set_video_id
        })
    }
    function rate(value) {
        Backend.rateSong(row.video_id, row.rating === value ? "INDIFFERENT" : value)
    }
    function addToQueue(index) {
        Backend.queueAddRow(row.rowJson(), index)
        Backend.showToast(index === 0 ? qsTr("Playing next") : qsTr("Added to queue"))
    }

    HoverHandler { id: hover }

    Rectangle {
        anchors.fill: parent
        // Card radius (28) minus its 8px inset.
        radius: 20
        color: row.isCurrent ? Theme.secondaryContainer : "transparent"
        Behavior on color { enabled: !Backend.lowPerformance; ColorAnimation { duration: 200 } }
        Rectangle {
            anchors.fill: parent
            radius: parent.radius
            color: row.isCurrent ? Theme.onSecondaryContainer : Theme.onSurface
            opacity: area.pressed ? 0.12 : hover.hovered ? 0.08 : 0
            Behavior on opacity { enabled: !Backend.lowPerformance; NumberAnimation { duration: 200 } }
        }
    }

    // Click plays; a sideways drag (vertical moves still scroll the list)
    // starts a drag to the queue panel.
    MouseArea {
        id: area
        property real pressX: 0
        property bool dragging: false
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onPressed: (mouse) => {
            pressX = mouse.x
            dragging = false
            if (row.draggable) chip.grabToImage(function(result) { row.Drag.imageSource = result.url })
        }
        onPositionChanged: (mouse) => {
            if (row.draggable && !dragging && Math.abs(mouse.x - pressX) > 12) {
                dragging = true
                row.Drag.active = true
            }
        }
        onClicked: if (!dragging && row.playHandler) row.playHandler()
    }
    Drag.dragType: Drag.Automatic
    Drag.supportedActions: Qt.CopyAction
    Drag.mimeData: ({ "text/x-spinwave-row": row.rowJson() })
    Drag.hotSpot.x: 16
    Drag.hotSpot.y: 16
    Drag.onDragFinished: row.Drag.active = false

    // Drag image: a title chip, rendered off-screen.
    Rectangle {
        id: chip
        x: -10000
        width: Math.min(260, chipText.implicitWidth + 32)
        height: 36
        radius: 18
        color: Theme.secondaryContainer
        Text {
            id: chipText
            anchors.centerIn: parent
            width: parent.width - 32
            text: row.title
            color: Theme.onSecondaryContainer
            font.family: "Google Sans Flex"
            font.pixelSize: 14
            font.weight: Font.Medium
            elide: Text.ElideRight
        }
    }

    component Link: Text {
        id: link
        property bool active: false
        signal activated()
        color: row.subFg
        font.family: "Roboto"
        font.pixelSize: 13
        font.underline: active && linkArea.containsMouse
        elide: Text.ElideRight
        MouseArea {
            id: linkArea
            anchors.fill: parent
            enabled: link.active
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: link.activated()
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 8
        anchors.rightMargin: 8
        spacing: 12

        // Lead: cover (or number) with the equalizer over the current song.
        Item {
            Layout.preferredWidth: 40
            Layout.preferredHeight: 40
            readonly property bool useCover: !row.showNumber && row.thumbnail_url.length > 0
            Rectangle {
                anchors.fill: parent
                visible: parent.useCover
                radius: 8
                color: Theme.surfaceContainerHighest
                RoundImage {
                    anchors.fill: parent
                    radius: 8
                    sourceSize: Qt.size(80, 80)
                    source: parent.visible ? Backend.smallThumb(row.thumbnail_url) : ""
                }
                Rectangle {
                    anchors.fill: parent
                    radius: 8
                    visible: row.isCurrent
                    color: "black"
                    opacity: 0.45
                }
            }
            EqBars {
                anchors.centerIn: parent
                visible: row.isCurrent
                barColor: parent.useCover ? "white" : Theme.primary
                running: Backend.isPlaying && !Backend.lowPerformance
            }
            Text {
                anchors.centerIn: parent
                visible: !row.isCurrent && !parent.useCover
                text: row.rowIndex + 1 < 10 ? "0" + (row.rowIndex + 1) : "" + (row.rowIndex + 1)
                color: row.subFg
                font.family: "Google Sans Flex"
                font.pixelSize: 14
                font.features: { "tnum": 1 }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignVCenter
            spacing: 0
            Text {
                Layout.fillWidth: true
                text: row.title
                color: row.isCurrent ? Theme.onSecondaryContainer : Theme.onSurface
                font.family: "Google Sans Flex"
                font.pixelSize: 16
                font.weight: row.isCurrent ? Font.Bold : Font.Medium
                elide: Text.ElideRight
            }
            // Artist · album, each a link when its page is known.
            Item {
                id: subLine
                Layout.fillWidth: true
                visible: row.artist.length > 0 || row.album.length > 0
                implicitHeight: artistLink.implicitHeight
                Row {
                    width: parent.width
                    Link {
                        id: artistLink
                        visible: row.artist.length > 0
                        text: row.artist
                        width: Math.min(implicitWidth, albumLink.visible ? subLine.width * 0.55 : subLine.width)
                        active: row.artist_id.length > 0
                        onActivated: Backend.openPage("artist", row.artist_id, row.artist)
                    }
                    Text {
                        id: dot
                        visible: artistLink.visible && albumLink.visible
                        text: " · "
                        color: row.subFg
                        font.family: "Roboto"
                        font.pixelSize: 13
                    }
                    Link {
                        id: albumLink
                        visible: row.album.length > 0
                        text: row.album
                        width: Math.max(0, Math.min(implicitWidth, subLine.width - (artistLink.visible ? artistLink.width + dot.width : 0)))
                        active: row.album_id.length > 0
                        onActivated: Backend.openPage("album", row.album_id, row.album)
                    }
                }
            }
        }

        // Hover actions; the more button stays for compact rows.
        Row {
            visible: row.showActions
            spacing: 2
            IconButton {
                visible: !row.compact
                implicitWidth: 34; implicitHeight: 34
                icon: "thumb_up"
                iconSize: 18
                label: row.rating === "LIKE" ? qsTr("Remove like") : qsTr("Like")
                filled: row.rating === "LIKE"
                fg: row.rating === "LIKE" ? Theme.primary : row.subFg
                enabled: Backend.isLoggedIn
                onClicked: row.rate("LIKE")
            }
            IconButton {
                visible: !row.compact
                implicitWidth: 34; implicitHeight: 34
                icon: "thumb_down"
                iconSize: 18
                label: row.rating === "DISLIKE" ? qsTr("Remove dislike") : qsTr("Dislike")
                filled: row.rating === "DISLIKE"
                fg: row.rating === "DISLIKE" ? Theme.primary : row.subFg
                enabled: Backend.isLoggedIn
                onClicked: row.rate("DISLIKE")
            }
            IconButton {
                visible: !row.compact
                implicitWidth: 34; implicitHeight: 34
                icon: "playlist_add"
                iconSize: 18
                label: qsTr("Add to queue")
                fg: row.subFg
                onClicked: row.addToQueue(-1)
            }
            IconButton {
                id: moreButton
                implicitWidth: 34; implicitHeight: 34
                icon: "more_vert"
                iconSize: 18
                label: qsTr("More actions")
                fg: row.subFg
                onClicked: {
                    menuLoader.active = true
                    menuLoader.item.menu.popup(moreButton, 0, moreButton.height)
                }
            }
        }

        // Liked songs keep a small mark while the actions are hidden.
        Icon {
            visible: !row.showActions && row.rating === "LIKE"
            name: "thumb_up"
            filled: true
            size: 18
            color: row.isCurrent ? Theme.onSecondaryContainer : Theme.primary
            Accessible.ignored: false
            Accessible.name: qsTr("Liked")
        }

        Text {
            Layout.preferredWidth: 44
            visible: !row.compact || !row.showActions
            text: row.duration
            color: row.subFg
            horizontalAlignment: Text.AlignRight
            font.family: "Roboto"
            font.pixelSize: 14
            font.features: { "tnum": 1 }
        }
    }

    // Built on first use: a list of hundreds of rows keeps no menus around.
    Loader {
        id: menuLoader
        active: false
        sourceComponent: Item {
            property alias menu: menu
            // Clipboard access for Share (QML has no clipboard API).
            TextEdit { id: clipboard; visible: false }
            M3Menu {
                id: menu
                onClosed: menuLoader.active = false

                M3MenuItem {
                    visible: row.compact
                    text: row.rating === "LIKE" ? qsTr("Remove like") : qsTr("Like")
                    glyph: "thumb_up"
                    enabled: Backend.isLoggedIn
                    onTriggered: row.rate("LIKE")
                }
                M3MenuItem {
                    visible: row.compact
                    text: row.rating === "DISLIKE" ? qsTr("Remove dislike") : qsTr("Dislike")
                    glyph: "thumb_down"
                    enabled: Backend.isLoggedIn
                    onTriggered: row.rate("DISLIKE")
                }
                M3MenuItem {
                    visible: row.compact
                    text: qsTr("Add to queue")
                    glyph: "playlist_add"
                    onTriggered: row.addToQueue(-1)
                }
                M3MenuItem {
                    text: qsTr("Play after current song")
                    glyph: "queue_play_next"
                    onTriggered: row.addToQueue(0)
                }
                M3Menu {
                    id: playlistMenu
                    title: qsTr("Add song to playlist")
                    // Shown as this menu's row in the parent (see delegate).
                    property string glyph: "playlist_add"
                    enabled: Backend.isLoggedIn
                    property var playlists: []
                    onAboutToShow: {
                        playlists = JSON.parse(Backend.editablePlaylistsJson() || "[]")
                        // Not loaded yet: fetch now so the next open lists them.
                        if (playlists.length === 0) Backend.startLibraryLoad(Backend.gateStatus)
                    }
                    M3MenuItem {
                        visible: playlistMenu.playlists.length === 0
                        enabled: false
                        text: qsTr("Loading your playlists…")
                        glyph: "hourglass_empty"
                    }
                    Instantiator {
                        model: playlistMenu.playlists
                        delegate: M3MenuItem {
                            required property var modelData
                            text: modelData.title
                            glyph: "queue_music"
                            onTriggered: Backend.addToPlaylist(row.video_id, modelData.playlist_id, modelData.title)
                        }
                        onObjectAdded: (index, object) => playlistMenu.insertItem(index + 1, object)
                        onObjectRemoved: (index, object) => playlistMenu.removeItem(object)
                    }
                }
                M3MenuItem {
                    text: qsTr("Share")
                    glyph: "share"
                    onTriggered: {
                        clipboard.text = Backend.shareLink(row.video_id)
                        clipboard.selectAll()
                        clipboard.copy()
                        Backend.showToast(qsTr("Song link copied to clipboard"))
                    }
                }
                M3MenuItem {
                    // Only for the song playing from one of your own playlists.
                    visible: Backend.canRemoveFromPlaylist(row.video_id)
                    text: qsTr("Delete from playlist")
                    glyph: "delete"
                    danger: true
                    onTriggered: Backend.removeFromPlaylist(row.video_id)
                }
            }
        }
    }

    Accessible.role: Accessible.Button
    Accessible.name: row.title + (row.artist.length > 0 ? ", " + row.artist : "")
    Accessible.onPressAction: if (row.playHandler) row.playHandler()
}
